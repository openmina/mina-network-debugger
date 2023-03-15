#![cfg_attr(feature = "kern", no_std, no_main, feature(lang_items))]

#[cfg(feature = "kern")]
use ebpf_kern as ebpf;
#[cfg(feature = "user")]
use ebpf_user as ebpf;

#[cfg(feature = "kern")]
ebpf::license!("GPL");

#[cfg(any(feature = "kern", feature = "user"))]
#[derive(ebpf::BpfApp)]
pub struct App {
    // output channel
    #[ringbuf(size = 0x8000000)]
    pub event_queue: ebpf::RingBufferRef,
    // track relevant pids
    // 0x1000 processes maximum
    #[hashmap(size = 0x1000)]
    pub pid: ebpf::HashMapRef<4, 4>,
    #[prog("tracepoint/syscalls/sys_enter_execve")]
    pub execve: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_execveat")]
    pub execveat: ebpf::ProgRef,
    // store/load context parameters
    // 0x100 cpus maximum
    #[hashmap(size = 0x100)]
    pub context_parameters: ebpf::HashMapRef<4, 0x20>,
    #[prog("tracepoint/syscalls/sys_enter_bind")]
    pub enter_bind: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_bind")]
    pub exit_bind: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_connect")]
    pub enter_connect: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_connect")]
    pub exit_connect: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_getsockopt")]
    pub enter_getsockopt: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_getsockopt")]
    pub exit_getsockopt: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_accept4")]
    pub enter_accept4: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_accept4")]
    pub exit_accept4: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_close")]
    pub enter_close: ebpf::ProgRef,
    // #[prog("tracepoint/syscalls/sys_exit_socket")]
    // pub exit_socket: ebpf::ProgRef,
    // #[prog("tracepoint/syscalls/sys_exit_open")]
    // pub exit_open: ebpf::ProgRef,
    // 0x4000 simultaneous connections maximum
    #[hashmap(size = 0x4000)]
    pub connections: ebpf::HashMapRef<8, 4>,
    #[prog("tracepoint/syscalls/sys_enter_write")]
    pub enter_write: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_write")]
    pub exit_write: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_read")]
    pub enter_read: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_read")]
    pub exit_read: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_sendto")]
    pub enter_sendto: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_sendto")]
    pub exit_sendto: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_recvfrom")]
    pub enter_recvfrom: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_recvfrom")]
    pub exit_recvfrom: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_getrandom")]
    pub enter_getrandom: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_getrandom")]
    pub exit_getrandom: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_shutdown")]
    pub enter_shutdown: ebpf::ProgRef,
}

#[cfg(feature = "kern")]
mod context;

#[cfg(feature = "kern")]
mod send;

#[cfg(feature = "kern")]
use bpf_recorder::{DataTag, Event};

#[cfg(feature = "kern")]
impl App {
    #[inline(always)]
    fn check_pid(&self) -> Result<(), i32> {
        use ebpf::helpers;

        let x = unsafe { helpers::get_current_pid_tgid() };
        let pid = (x >> 32) as u32;

        if let Some(&flags) = self.pid.get(&pid.to_ne_bytes()) {
            let flags = u32::from_ne_bytes(flags);

            if flags == 0xffffffff {
                return Ok(());
            }
        }

        Err(0)
    }

    #[allow(clippy::nonminimal_bool)]
    #[inline(never)]
    fn check_env_entry(&mut self, entry: *const u8) -> Result<u32, i32> {
        use ebpf::helpers;

        let mut str_bytes = self.event_queue.reserve(0x200)?;
        let c = unsafe {
            helpers::probe_read_user_str(str_bytes.as_mut().as_mut_ptr() as _, 0x200, entry as _)
        };

        // Too short or too long
        if !(9..=0x200).contains(&c) {
            str_bytes.discard();
            return Err(c as _);
        }
        // Prefix is 'BPF_ALIAS'
        let prefix = true
            && str_bytes.as_ref()[0] == b'B'
            && str_bytes.as_ref()[1] == b'P'
            && str_bytes.as_ref()[2] == b'F'
            && str_bytes.as_ref()[3] == b'_'
            && str_bytes.as_ref()[4] == b'A'
            && str_bytes.as_ref()[5] == b'L'
            && str_bytes.as_ref()[6] == b'I'
            && str_bytes.as_ref()[7] == b'A'
            && str_bytes.as_ref()[8] == b'S';

        str_bytes.discard();
        if prefix {
            Ok((c - 10) as u32)
        } else {
            Err(0)
        }
    }

    fn check_env_flag(&mut self, env: *const *const u8) -> Result<(), i32> {
        use ebpf::helpers;

        if env.is_null() {
            return Err(0);
        }

        let mut i = 0;
        let mut env_str = self.event_queue.reserve(8)?;
        loop {
            let c = unsafe {
                helpers::probe_read_user(env_str.as_mut().as_mut_ptr() as _, 8, env.offset(i) as _)
            };

            if c != 0 {
                break;
            }
            i += 1;

            let entry = unsafe { *(env_str.as_ref().as_ptr() as *const *const u8) };
            if entry.is_null() {
                break;
            }

            if let Ok(len) = self.check_env_entry(entry) {
                env_str.discard();
                let (pid, tid) = {
                    let x = unsafe { helpers::get_current_pid_tgid() };
                    ((x >> 32) as u32, (x & 0xffffffff) as u32)
                };

                let ts = unsafe { helpers::ktime_get_boot_ns() };
                let event = Event::new(pid, tid, ts, ts);
                let event = event.set_tag_fd(DataTag::Alias, 0).set_ok(len as u64);
                let name = unsafe { entry.offset(10) };
                send::dyn_sized::<typenum::B0>(&mut self.event_queue, event, name)?;

                return self
                    .pid
                    .insert(pid.to_ne_bytes(), 0x_ffff_ffff_u32.to_ne_bytes());
            }
            if i >= 0x100 {
                break;
            }
        }
        env_str.discard();

        Err(0)
    }

    #[inline(never)]
    fn check_arg_entry(&mut self, entry: *const u8) -> Result<(), i32> {
        use ebpf::helpers;

        let mut str_bytes = self.event_queue.reserve(0x200)?;
        let c = unsafe {
            helpers::probe_read_user_str(str_bytes.as_mut().as_mut_ptr() as _, 0x200, entry as _)
        };

        // Too short or too long
        if !(18..=0x200).contains(&c) {
            str_bytes.discard();
            return Err(c as _);
        }
        // Prefix is 'coda-libp2p_helper'
        let prefix = true
            && str_bytes.as_ref()[0] == b'c'
            && str_bytes.as_ref()[1] == b'o'
            && str_bytes.as_ref()[2] == b'd'
            && str_bytes.as_ref()[3] == b'a'
            && str_bytes.as_ref()[4] == b'-'
            && str_bytes.as_ref()[5] == b'l'
            && str_bytes.as_ref()[6] == b'i'
            && str_bytes.as_ref()[7] == b'b'
            && str_bytes.as_ref()[8] == b'p'
            && str_bytes.as_ref()[9] == b'2'
            && str_bytes.as_ref()[10] == b'p'
            && str_bytes.as_ref()[11] == b'_'
            && str_bytes.as_ref()[12] == b'h'
            && str_bytes.as_ref()[13] == b'e'
            && str_bytes.as_ref()[14] == b'l'
            && str_bytes.as_ref()[15] == b'p'
            && str_bytes.as_ref()[16] == b'e'
            && str_bytes.as_ref()[17] == b'r';

        str_bytes.discard();
        if prefix {
            Ok(())
        } else {
            Err(0)
        }
    }

    fn check_name(&mut self, argv: *const *const u8) -> Result<(), i32> {
        use ebpf::helpers;

        if argv.is_null() {
            return Err(0);
        }

        let mut arg_str = self.event_queue.reserve(8)?;
        loop {
            let c = unsafe {
                helpers::probe_read_user(arg_str.as_mut().as_mut_ptr() as _, 8, argv as _)
            };

            if c != 0 {
                break;
            }

            let entry = unsafe { *(arg_str.as_ref().as_ptr() as *const *const u8) };
            if entry.is_null() {
                break;
            }

            if let Ok(()) = self.check_arg_entry(entry) {
                arg_str.discard();

                return Ok(());
            }

            // only first argument is relevant
            break;
        }

        arg_str.discard();

        Err(0)
    }

    #[inline(always)]
    pub fn execve(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        let argv = ctx.read_here::<*const *const u8>(0x18);
        self.check_name(argv)?;
        let env = ctx.read_here::<*const *const u8>(0x20);
        self.check_env_flag(env)
    }

    #[inline(always)]
    pub fn execveat(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        let argv = ctx.read_here::<*const *const u8>(0x20);
        self.check_name(argv)?;
        let env = ctx.read_here::<*const *const u8>(0x28);
        self.check_env_flag(env)
    }

    #[inline(always)]
    fn enter(&mut self, data: context::Variant) -> Result<(), i32> {
        use core::{mem, ptr};
        use ebpf::helpers;

        self.check_pid()?;
        let (_, thread_id) = {
            let x = unsafe { helpers::get_current_pid_tgid() };
            ((x >> 32) as u32, (x & 0xffffffff) as u32)
        };
        let ts = unsafe { helpers::ktime_get_boot_ns() };

        let mut context = context::Parameters {
            data: context::Variant::Empty { ptr: 0, len: 0 },
            ts,
        };
        // bpf validator forbids reading from stack uninitialized data
        // different variants of this enum has different length,
        unsafe { ptr::write_volatile(&mut context.data, mem::zeroed()) };
        context.data = data;

        self.context_parameters
            .insert_unsafe(thread_id.to_ne_bytes(), context)
    }

    #[inline(always)]
    fn exit(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        use ebpf::helpers;

        self.check_pid()?;
        let (_, thread_id) = {
            let x = unsafe { helpers::get_current_pid_tgid() };
            ((x >> 32) as u32, (x & 0xffffffff) as u32)
        };
        let ts1 = unsafe { helpers::ktime_get_boot_ns() };

        match self
            .context_parameters
            .remove_unsafe::<context::Parameters>(&thread_id.to_ne_bytes())?
        {
            Some(context::Parameters { data, ts: ts0 }) => {
                let ret = ctx.read_here(0x10);
                self.on_ret(ret, data, ts0, ts1)
            }
            None => Err(-1),
        }
    }

    #[inline(never)]
    fn on_ret(&mut self, ret: i64, data: context::Variant, ts0: u64, ts1: u64) -> Result<(), i32> {
        use core::ptr;
        use ebpf::helpers;

        const EAGAIN: i64 = -11;
        if ret == EAGAIN {
            return Ok(());
        }

        fn check_addr(ptr: *const u8) -> Result<(), i32> {
            const AF_INET: u16 = 2;
            const AF_INET6: u16 = 10;

            let mut ty = 0u16;
            let c = unsafe { helpers::probe_read_user((&mut ty) as *mut _ as _, 2, ptr as _) };
            if c != 0 {
                // cannot read first two bytes of the address
                return Err(0);
            }
            if ty == AF_INET {
                let mut ip = [0u8; 4];
                let c = unsafe {
                    helpers::probe_read_user(ip.as_mut_ptr() as *mut _, 4, ptr.offset(4) as _)
                };
                if c != 0 {
                    return Err(0);
                }

                // if ip[0] == 127 && ip[1] == 0 && ip[2] == 0 {
                //     return Err(0);
                // }
            } else if ty == AF_INET6 {
                // filter there something
            } else {
                // ignore everything else
                return Err(0);
            }

            let mut port = 0u16;
            let c = unsafe {
                helpers::probe_read_user((&mut port) as *mut _ as _, 2, ptr.offset(2) as _)
            };
            if c != 0 {
                return Err(0);
            }
            if matches!(ty, 0 | 53 | 443 | 80 | 65535) {
                return Err(0);
            }

            Ok(())
        }

        let (pid, tid) = {
            let x = unsafe { helpers::get_current_pid_tgid() };
            ((x >> 32) as u32, (x & 0xffffffff) as u32)
        };

        let event = Event::new(pid, tid, ts0, ts1);
        let ptr = data.ptr();
        let event = match data {
            context::Variant::Empty { len, .. } => {
                let event = event.set_tag_fd(DataTag::Debug, 0);
                if ret < 0 {
                    event.set_err(ret)
                } else {
                    event.set_ok(len)
                }
            }
            context::Variant::Bind { fd, addr_len, .. } => {
                let event = event.set_tag_fd(DataTag::Bind, fd);
                if ret < 0 {
                    event.set_err(ret)
                } else {
                    event.set_ok(addr_len)
                }
            }
            context::Variant::Connect { fd, addr_len, .. } => {
                check_addr(ptr)?;

                const EINPROGRESS: i64 = -115;
                let event = event.set_tag_fd(DataTag::Connect, fd);
                if ret < 0 && ret != EINPROGRESS {
                    event.set_err(ret)
                } else {
                    let socket_id = ((fd as u64) << 32) + (pid as u64);
                    self.connections
                        .insert(socket_id.to_ne_bytes(), 0x1_u32.to_ne_bytes())?;
                    event.set_ok(addr_len)
                }
            }
            context::Variant::GetSockOptL1O4 { fd, len_ptr, .. } => {
                let event = event.set_tag_fd(DataTag::GetSockOpt, fd);
                if ret < 0 {
                    event.set_err(ret)
                } else {
                    let mut len_bytes = [0_u8; 4];
                    let c = unsafe {
                        let p = len_bytes.as_mut_ptr() as *mut _;
                        helpers::probe_read_user(p, 4, len_ptr as _)
                    };
                    if c != 0 {
                        return Err(0);
                    }
                    let len = u32::from_ne_bytes(len_bytes) as u64;
                    event.set_ok(len)
                }
            }
            context::Variant::GetSockOptIrrelevant { .. } => {
                return Ok(());
            }
            context::Variant::Accept {
                listen_on_fd,
                addr_len_ptr,
                ..
            } => {
                let _ = listen_on_fd;
                let fd = ret as _;
                let event = event.set_tag_fd(DataTag::Accept, fd);
                if ret < 0 {
                    event.set_err(ret)
                } else {
                    check_addr(ptr)?;
                    let socket_id = ((fd as u64) << 32) + (pid as u64);
                    self.connections
                        .insert(socket_id.to_ne_bytes(), 0x1_u32.to_ne_bytes())?;

                    let mut addr_len_bytes = [0_u8; 4];
                    let c = unsafe {
                        let p = addr_len_bytes.as_mut_ptr() as *mut _;
                        helpers::probe_read_user(p, 4, addr_len_ptr as _)
                    };
                    if c != 0 {
                        return Err(0);
                    }
                    let addr_len = u32::from_ne_bytes(addr_len_bytes) as u64;

                    event.set_ok(addr_len)
                }
            }
            context::Variant::Send { fd, .. } | context::Variant::Write { fd, .. } => {
                let event = event.set_tag_fd(DataTag::Write, fd);
                if fd == 0 || fd == 1 || fd == 2 {
                    if ret >= 0 {
                        event.set_ok(ret as _)
                    } else {
                        return Ok(());
                    }
                } else {
                    let socket_id = ((fd as u64) << 32) + (pid as u64);
                    if self.connections.get(&socket_id.to_ne_bytes()).is_none() {
                        return Ok(());
                    }
                    if ret < 0 {
                        if self.connections.remove(&socket_id.to_ne_bytes())?.is_none() {
                            return Ok(());
                        }
                        let close_ev = event.set_tag_fd(DataTag::Close, fd);
                        let event = event.set_err(ret);
                        send::dyn_sized::<typenum::B0>(&mut self.event_queue, event, ptr::null())?;
                        close_ev
                    } else {
                        event.set_ok(ret as _)
                    }
                }
            }
            context::Variant::Recv { fd, .. } | context::Variant::Read { fd, .. } => {
                let event = event.set_tag_fd(DataTag::Read, fd);
                if fd == 0 || fd == 1 || fd == 2 {
                    if ret >= 0 {
                        event.set_ok(ret as _)
                    } else {
                        return Ok(());
                    }
                } else {
                    let socket_id = ((fd as u64) << 32) + (pid as u64);
                    if self.connections.get(&socket_id.to_ne_bytes()).is_none() {
                        return Ok(());
                    }
                    if ret < 0 {
                        if self.connections.remove(&socket_id.to_ne_bytes())?.is_none() {
                            return Ok(());
                        }
                        let close_ev = event.set_tag_fd(DataTag::Close, fd);
                        let event = event.set_err(ret);
                        send::dyn_sized::<typenum::B0>(&mut self.event_queue, event, ptr::null())?;
                        close_ev
                    } else {
                        event.set_ok(ret as _)
                    }
                }
            }
            context::Variant::GetRandom { data_len, .. } => {
                event.set_tag_fd(DataTag::Random, 0).set_ok(data_len)
            }
        };
        send::dyn_sized::<typenum::B0>(&mut self.event_queue, event, ptr)
    }

    #[inline(always)]
    pub fn enter_bind(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.enter(context::Variant::Bind {
            fd: ctx.read_here::<u64>(0x10) as u32,
            addr_ptr: ctx.read_here::<u64>(0x18),
            addr_len: ctx.read_here::<u64>(0x20),
        })
    }

    #[inline(always)]
    pub fn exit_bind(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.exit(ctx)
    }

    #[inline(always)]
    pub fn enter_connect(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.enter(context::Variant::Connect {
            fd: ctx.read_here::<u64>(0x10) as u32,
            addr_ptr: ctx.read_here::<u64>(0x18),
            addr_len: ctx.read_here::<u64>(0x20),
        })
    }

    #[inline(always)]
    pub fn exit_connect(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.exit(ctx)
    }

    #[inline(always)]
    pub fn enter_getsockopt(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        let level = ctx.read_here::<u64>(0x18);
        let opt = ctx.read_here::<u64>(0x20);
        if level == 1 && opt == 4 {
            self.enter(context::Variant::GetSockOptL1O4 {
                fd: ctx.read_here::<u64>(0x10) as u32,
                val_ptr: ctx.read_here::<u64>(0x28),
                len_ptr: ctx.read_here::<u64>(0x30),
            })
        } else {
            self.enter(context::Variant::GetSockOptIrrelevant {
                fd: ctx.read_here::<u64>(0x10) as u32,
                val_ptr: ctx.read_here::<u64>(0x28),
                len_ptr: ctx.read_here::<u64>(0x30),
            })
        }
    }

    #[inline(always)]
    pub fn exit_getsockopt(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.exit(ctx)
    }

    #[inline(always)]
    pub fn enter_accept4(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.enter(context::Variant::Accept {
            listen_on_fd: ctx.read_here::<u64>(0x10) as u32,
            addr_ptr: ctx.read_here::<u64>(0x18),
            addr_len_ptr: ctx.read_here::<u64>(0x20),
        })
    }

    #[inline(always)]
    pub fn exit_accept4(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.exit(ctx)
    }

    #[inline(always)]
    pub fn enter_close(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        use core::ptr;
        use ebpf::helpers;

        self.check_pid()?;

        let fd = ctx.read_here::<u64>(0x10) as u32;
        let (pid, tid) = {
            let x = unsafe { helpers::get_current_pid_tgid() };
            ((x >> 32) as u32, (x & 0xffffffff) as u32)
        };
        let ts = unsafe { helpers::ktime_get_boot_ns() };

        let socket_id = ((fd as u64) << 32) + (pid as u64);
        if self.connections.remove(&socket_id.to_ne_bytes())?.is_none() {
            return Ok(());
        }

        let event = Event::new(pid, tid, ts, ts);
        let event = event.set_tag_fd(DataTag::Close, fd);
        send::dyn_sized::<typenum::B0>(&mut self.event_queue, event, ptr::null())
    }

    // #[inline(always)]
    // pub fn exit_socket(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
    //     self.enter_close(ctx)
    // }

    // #[inline(always)]
    // pub fn exit_open(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
    //     self.enter_close(ctx)
    // }

    #[inline(always)]
    pub fn enter_write(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.enter(context::Variant::Write {
            fd: ctx.read_here::<u64>(0x10) as u32,
            data_ptr: ctx.read_here::<u64>(0x18),
            _pad: 0,
        })
    }

    #[inline(always)]
    pub fn exit_write(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.exit(ctx)
    }

    #[inline(always)]
    pub fn enter_read(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.enter(context::Variant::Read {
            fd: ctx.read_here::<u64>(0x10) as u32,
            data_ptr: ctx.read_here::<u64>(0x18),
            _pad: 0,
        })
    }

    #[inline(always)]
    pub fn exit_read(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.exit(ctx)
    }

    #[inline(always)]
    pub fn enter_sendto(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.enter(context::Variant::Send {
            fd: ctx.read_here::<u64>(0x10) as u32,
            data_ptr: ctx.read_here::<u64>(0x18),
            _pad: 0,
        })
    }

    #[inline(always)]
    pub fn exit_sendto(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.exit(ctx)
    }

    #[inline(always)]
    pub fn enter_recvfrom(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.enter(context::Variant::Recv {
            fd: ctx.read_here::<u64>(0x10) as u32,
            data_ptr: ctx.read_here::<u64>(0x18),
            _pad: 0,
        })
    }

    #[inline(always)]
    pub fn exit_recvfrom(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.exit(ctx)
    }

    #[inline(always)]
    pub fn enter_getrandom(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        let len = ctx.read_here::<u64>(0x18);
        self.enter(context::Variant::GetRandom {
            _fd: 0,
            data_ptr: ctx.read_here::<u64>(0x10),
            data_len: len,
        })
    }

    #[inline(always)]
    pub fn exit_getrandom(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.exit(ctx)
    }

    #[inline(always)]
    pub fn enter_shutdown(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.enter_close(ctx)
    }
}

#[cfg(feature = "user")]
fn main() {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{SystemTime, Duration},
        env, thread,
        path::PathBuf,
    };

    use bpf_recorder::{
        sniffer_event::{SnifferEvent, SnifferEventVariant},
        proc,
    };
    use simulator::registry::messages::{DebuggerReport, ConnectionMetadata};
    use bpf_ring_buffer::RingBuffer;
    use mina_recorder::{
        EventMetadata, ConnectionInfo, server, P2pRecorder, libp2p_helper::CapnpReader,
    };
    use ebpf::{kind::AppItem, Skeleton, SkeletonEmpty};

    fn watch_pid(pid: u32, terminating: Arc<AtomicBool>) {
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            if !proc::cmd_prefix_matches(pid, "coda-libp2p_helper").unwrap_or_default() {
                terminating.store(true, Ordering::SeqCst);
                break;
            }
        });
    }

    // let env = env_logger::Env::default().default_filter_or("warn");
    // env_logger::init_from_env(env);
    // if let Err(err) = sudo::escalate_if_needed() {
    //     log::error!("failed to obtain superuser permission {err}");
    //     return;
    // }

    let port = env::var("SERVER_PORT")
        .unwrap_or_else(|_| 8000.to_string())
        .parse()
        .unwrap_or(8000);
    let db_path = env::var("DB_PATH").unwrap_or_else(|_| "target/db".to_string());
    let db_path = PathBuf::from(db_path);
    let dry = env::var("DRY").is_ok();

    let key_path = env::var("HTTPS_KEY_PATH").ok();
    let cert_path = env::var("HTTPS_CERT_PATH").ok();

    // TODO: fix logging in file
    // let log = File::create(db_path.join("log")).expect("cannot create log file");
    // let mut builder = env_logger::Builder::new();
    // builder.target(env_logger::Target::Pipe(Box::new(log)));
    // builder.try_init().expect("cannot setup logging");
    env_logger::init();

    let (db, callback, server_thread) = server::spawn(port, db_path, key_path, cert_path);
    let terminating = Arc::new(AtomicBool::new(dry));
    {
        let terminating = terminating.clone();
        let mut callback = Some(callback);
        let user_handler = move || {
            log::info!("ctrlc");
            if let Some(cb) = callback.take() {
                cb();
            }
            terminating.store(true, Ordering::SeqCst);
        };
        if let Err(err) = ctrlc::set_handler(user_handler) {
            log::error!("failed to set ctrlc handler {err}");
            return;
        }
    }

    let db_capnp = db.core();

    struct Source {
        _skeleton: SkeletonEmpty,
        _app: Box<App>,
        rb: RingBuffer,
        terminating: Arc<AtomicBool>,
    }

    impl Source {
        pub fn initialize(terminating: Arc<AtomicBool>) -> Self {
            static CODE: &[u8] = include_bytes!(concat!("../", env!("BPF_CODE_RECORDER")));

            let mut skeleton = Skeleton::<App>::open("bpf-recorder\0", CODE)
                .unwrap_or_else(|code| panic!("failed to open bpf: {}", code));
            skeleton
                .load()
                .unwrap_or_else(|code| panic!("failed to load bpf: {}", code));
            let (skeleton, mut app) = skeleton
                .attach()
                .unwrap_or_else(|code| panic!("failed to attach bpf: {}", code));
            log::info!("attached bpf module");

            let fd = match app.event_queue.kind_mut() {
                ebpf::kind::AppItemKindMut::Map(map) => map.fd(),
                _ => unreachable!(),
            };

            let mut info = libbpf_sys::bpf_map_info::default();
            let mut len = std::mem::size_of::<libbpf_sys::bpf_map_info>() as u32;
            unsafe {
                libbpf_sys::bpf_obj_get_info_by_fd(
                    fd,
                    &mut info as *mut libbpf_sys::bpf_map_info as *mut _,
                    &mut len as _,
                )
            };
            let rb = match RingBuffer::new(fd, info.max_entries as usize) {
                Ok(v) => v,
                Err(err) => {
                    log::error!("failed to create userspace part of the ring buffer: {err}");
                    std::process::exit(1);
                }
            };

            Source {
                _skeleton: skeleton,
                _app: app,
                rb,
                terminating,
            }
        }
    }

    impl Iterator for Source {
        type Item = (Option<SnifferEvent>, usize);

        fn next(&mut self) -> Option<Self::Item> {
            self.rb
                .read_blocking::<SnifferEvent>(&self.terminating)
                .ok()
        }
    }

    let mut source = if dry {
        Box::new(std::iter::empty()) as Box<dyn Iterator<Item = (Option<SnifferEvent>, usize)>>
    } else {
        Box::new(Source::initialize(terminating.clone()))
            as Box<dyn Iterator<Item = (Option<SnifferEvent>, usize)>>
    };

    let test = env::var("TEST").is_ok();

    let mut origin = proc::S::read().ok().and_then(|s| s.b_time);
    if let Some(boot_time) = &origin {
        log::info!("boot time: {boot_time:?}");
    }

    let mut p2p_cns = BTreeMap::new();
    let counter = db.messages.clone();
    let mut pending_out_cns = BTreeMap::new();
    let mut recorder = P2pRecorder::new(db, test);
    let mut watching = BTreeMap::new();
    let mut capnp_readers = BTreeMap::<_, CapnpReader>::new();
    let mut capnp_blacklist = BTreeSet::new();
    let mut max_buffered = 0;
    let mut max_unordered_ns = BTreeMap::new();
    let mut last_ts = BTreeMap::new();
    let mut subscriptions = BTreeMap::new();
    let mut chain_id = BTreeMap::new();
    let mut max_lag = Duration::ZERO;
    while !terminating.load(Ordering::SeqCst) {
        for (event, buffered) in source.by_ref() {
            let event = match event {
                Some(v) => v,
                None => continue,
            };

            if buffered > max_buffered {
                max_buffered = buffered;
                log::info!("buffered data update maximum: {buffered}");
            }

            let last = last_ts.get(&event.tid).cloned().unwrap_or_default();
            if event.ts1 < last {
                let unordered = last - event.ts1;
                log::warn!(
                    "unordered {unordered}, {} < {last}, message id {}",
                    event.ts1,
                    counter.load(Ordering::Relaxed)
                );
                let max_unordered_ns = max_unordered_ns.entry(event.tid).or_default();
                if unordered > *max_unordered_ns {
                    *max_unordered_ns = unordered;
                }
            }
            last_ts.insert(event.tid, event.ts1);
            let time = match &origin {
                None => {
                    let now = SystemTime::now();
                    origin = Some(now - Duration::from_nanos(event.ts1));
                    now
                }
                Some(origin) => *origin + Duration::from_nanos(event.ts1),
            };
            let better_time = {
                let instant_there = Duration::from_nanos(event.ts1);
                let mut tp = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut tp) };
                let instant_here = Duration::new(tp.tv_sec as _, tp.tv_nsec as _);
                let delta = instant_here.checked_sub(instant_there).unwrap_or_default();
                if delta >= max_lag + Duration::from_secs(60) {
                    max_lag = delta;
                    log::warn!("lagging: {delta:?}");
                }
                SystemTime::now() - delta
            };
            let duration = Duration::from_nanos(event.ts1 - event.ts0);
            match event.variant {
                SnifferEventVariant::NewApp(alias) => {
                    log::info!("exec {alias} pid: {}", event.pid);
                    recorder.on_alias(event.pid, alias);
                    if !watching.contains_key(&event.pid) {
                        let version = env!("GIT_HASH");
                        watching.insert(
                            event.pid,
                            DebuggerReport {
                                version: version.to_owned(),
                                ipc: Default::default(),
                                network: vec![],
                            },
                        );
                        if env::var("TERMINATE").is_ok() {
                            watch_pid(event.pid, terminating.clone());
                        }
                    }
                }
                SnifferEventVariant::Bind(addr) => {
                    recorder.set_port(event.pid, addr.port());
                }
                SnifferEventVariant::OutgoingConnection(addr) => {
                    let metadata = EventMetadata {
                        id: ConnectionInfo {
                            addr,
                            pid: event.pid,
                            fd: event.fd,
                        },
                        time,
                        better_time,
                        duration,
                    };

                    log::info!("new unconfirmed {metadata}");
                    pending_out_cns.insert((event.pid, event.fd), addr);
                }
                SnifferEventVariant::GetSockOpt(value) => {
                    if value.len() != 4 {
                        continue;
                    }
                    let Some(addr) = pending_out_cns.remove(&(event.pid, event.fd)) else {
                        continue;
                    };
                    let metadata = EventMetadata {
                        id: ConnectionInfo {
                            addr,
                            pid: event.pid,
                            fd: event.fd,
                        },
                        time,
                        better_time,
                        duration,
                    };
                    let value = u32::from_ne_bytes(
                        value
                            .as_slice()
                            .try_into()
                            .expect("must be checked above `value.len() != 4`"),
                    );
                    log::info!("getsockopt {value}, {metadata}");
                    if value != 0 {
                        continue;
                    }
                    if let Some(report) = watching.get_mut(&event.pid) {
                        let counter = report
                            .network
                            .iter()
                            .filter(|cn| cn.ip == addr.ip())
                            .count();
                        report.network.push(ConnectionMetadata {
                            ip: addr.ip(),
                            counter,
                            incoming: false,
                            fd: event.fd as i32,
                            checksum: Default::default(),
                            timestamp: better_time,
                        });
                    }

                    if let Some(old_addr) = p2p_cns.insert((event.pid, event.fd), addr) {
                        log::warn!("new outgoing connection on already allocated fd");
                        let mut metadata = metadata.clone();
                        metadata.id.addr = old_addr;
                        recorder.on_disconnect(metadata, buffered);
                    }
                    log::info!("new outgoing connection {}", metadata);
                    recorder.on_connect::<true>(
                        false,
                        metadata,
                        buffered,
                        chain_id.get(&event.pid).cloned().unwrap_or_default(),
                    );
                }
                SnifferEventVariant::IncomingConnection(addr) => {
                    if let Some(report) = watching.get_mut(&event.pid) {
                        let counter = report
                            .network
                            .iter()
                            .filter(|cn| cn.ip == addr.ip())
                            .count();
                        report.network.push(ConnectionMetadata {
                            ip: addr.ip(),
                            counter,
                            incoming: true,
                            fd: event.fd as i32,
                            checksum: Default::default(),
                            timestamp: better_time,
                        });
                    }

                    let metadata = EventMetadata {
                        id: ConnectionInfo {
                            addr,
                            pid: event.pid,
                            fd: event.fd,
                        },
                        time,
                        better_time,
                        duration,
                    };
                    if let Some(old_addr) = p2p_cns.insert((event.pid, event.fd), addr) {
                        log::warn!("new incoming connection on already allocated fd");
                        let mut metadata = metadata.clone();
                        metadata.id.addr = old_addr;
                        recorder.on_disconnect(metadata, buffered);
                    }
                    log::info!("new incoming connection {}", metadata);
                    recorder.on_connect::<true>(
                        true,
                        metadata,
                        buffered,
                        chain_id.get(&event.pid).cloned().unwrap_or_default(),
                    );
                }
                SnifferEventVariant::Disconnected => {
                    let key = (event.pid, event.fd);
                    if let Some(addr) = p2p_cns.remove(&key) {
                        let metadata = EventMetadata {
                            id: ConnectionInfo {
                                addr,
                                pid: event.pid,
                                fd: event.fd,
                            },
                            time,
                            better_time,
                            duration,
                        };
                        log::info!("disconnected {}", metadata);
                        recorder.on_disconnect(metadata, buffered);
                    } else {
                        // `close` means close socket, not necessarily it was connected
                        // so it is ok
                        log::debug!(
                            "{} cannot process disconnect {}, not connected",
                            event.pid,
                            event.fd
                        );
                    }
                }
                SnifferEventVariant::Error(_, -104) => {}
                SnifferEventVariant::Error(tag, code) => {
                    let key = (event.pid, event.fd);
                    if let Some(addr) = p2p_cns.get(&key) {
                        let metadata = EventMetadata {
                            id: ConnectionInfo {
                                addr: *addr,
                                pid: event.pid,
                                fd: event.fd,
                            },
                            time,
                            better_time,
                            duration,
                        };

                        log::error!("{metadata},  tag: {tag:?}, code: {code}");
                    }
                }
                SnifferEventVariant::IncomingData(data) => {
                    if event.fd == 0 || event.fd == 1 {
                        watching
                            .get_mut(&event.pid)
                            .map(|report| report.ipc.0 += &data);

                        let key = (event.pid, true);
                        if capnp_blacklist.contains(&key) {
                            continue;
                        }
                        let reader = capnp_readers.entry(key).or_default();
                        reader.extend_from_slice(&data);
                        let local_node_address = recorder.cx.pid_to_addr(event.pid);
                        if !reader.process(
                            event.pid,
                            true,
                            local_node_address,
                            time,
                            better_time,
                            &db_capnp,
                            &mut subscriptions,
                            chain_id.entry(event.pid).or_default(),
                        ) {
                            capnp_readers.remove(&key);
                            capnp_blacklist.insert(key);
                        }
                        continue;
                    }
                    if event.fd == 2 {
                        // TODO:
                        continue;
                    }
                    let key = (event.pid, event.fd);
                    if let Some(addr) = p2p_cns.get(&key) {
                        watching
                            .get_mut(&event.pid)
                            .and_then(|report| {
                                report
                                    .network
                                    .iter_mut()
                                    .rev()
                                    .find(|cn| addr.ip() == cn.ip && event.fd == cn.fd as u32)
                            })
                            .map(|connection| connection.checksum.0 += &data);

                        let metadata = EventMetadata {
                            id: ConnectionInfo {
                                addr: *addr,
                                pid: event.pid,
                                fd: event.fd,
                            },
                            time,
                            better_time,
                            duration,
                        };
                        recorder.on_data(true, metadata, buffered, data);
                    } else {
                        log::warn!(
                            "{} cannot handle data on {}, not connected, {}",
                            event.pid,
                            event.fd,
                            hex::encode(data),
                        );
                    }
                }
                SnifferEventVariant::OutgoingData(data) => {
                    if event.fd == 0 || event.fd == 1 {
                        watching
                            .get_mut(&event.pid)
                            .map(|report| report.ipc.1 += &data);

                        let key = (event.pid, false);
                        if capnp_blacklist.contains(&key) {
                            continue;
                        }
                        let reader = capnp_readers.entry(key).or_default();
                        reader.extend_from_slice(&data);
                        let local_node_address = recorder.cx.pid_to_addr(event.pid);
                        if !reader.process(
                            event.pid,
                            false,
                            local_node_address,
                            time,
                            better_time,
                            &db_capnp,
                            &mut subscriptions,
                            chain_id.entry(event.pid).or_default(),
                        ) {
                            capnp_readers.remove(&key);
                            capnp_blacklist.insert(key);
                        }
                        continue;
                    }
                    if event.fd == 2 {
                        // TODO:
                        continue;
                    }
                    let key = (event.pid, event.fd);
                    if let Some(addr) = p2p_cns.get(&key) {
                        watching
                            .get_mut(&event.pid)
                            .and_then(|report| {
                                report
                                    .network
                                    .iter_mut()
                                    .rev()
                                    .find(|cn| addr.ip() == cn.ip && event.fd == cn.fd as u32)
                            })
                            .map(|connection| connection.checksum.1 += &data);
                        let metadata = EventMetadata {
                            id: ConnectionInfo {
                                addr: *addr,
                                pid: event.pid,
                                fd: event.fd,
                            },
                            time,
                            better_time,
                            duration,
                        };
                        recorder.on_data(false, metadata, buffered, data);
                    } else {
                        log::warn!(
                            "{} cannot handle data on {}, not connected, {}",
                            event.pid,
                            event.fd,
                            hex::encode(data),
                        );
                    }
                }
                SnifferEventVariant::Random(random) => {
                    recorder.on_randomness(event.pid, random, time);
                }
            }
        }
    }

    if let Ok(host) = env::var("REGISTRY") {
        if let Ok(client) = reqwest::blocking::ClientBuilder::new()
            .timeout(Duration::from_secs(30))
            .build()
        {
            let build_number = env::var("BUILD_NUMBER")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or_default();
            for (pid, report) in &watching {
                let summary_json = match serde_json::to_string(report) {
                    Ok(v) => v,
                    Err(err) => {
                        log::error!("cannot post summary for pid: {pid}, error: {err}");
                        continue;
                    }
                };

                const TRIES_NUMBER: usize = 60;
                let mut tries = TRIES_NUMBER;
                loop {
                    let r = client
                        .post(format!(
                            "http://{host}:80/report/debugger?build_number={build_number}"
                        ))
                        .body(summary_json.clone())
                        .send();
                    match r {
                        Ok(v) => {
                            drop(v.status());
                            break;
                        }
                        Err(err) => {
                            tries -= 1;
                            log::error!("try {}, {err}", TRIES_NUMBER - tries);
                            if tries == 0 {
                                break;
                            } else {
                                thread::sleep(Duration::from_secs(2));
                            }
                        }
                    }
                }
            }

            if env::var("DEBUGGER_WAIT_FOREVER").is_ok() {
                loop {
                    std::hint::spin_loop();
                    std::thread::yield_now();
                }
            }
        }
    }

    // TODO: investigate stuck
    // if server_thread.join().is_err() {
    //     log::error!("server thread panic, this is a bug, must not happen");
    // }
    let _ = server_thread;

    log::info!("terminated");
    drop(source);
}
