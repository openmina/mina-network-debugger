#![cfg_attr(
    feature = "kern",
    no_std,
    no_main,
    feature(lang_items),
    feature(core_ffi_c)
)]

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
    #[hashmap(size = 0x1000)]
    pub pid: ebpf::HashMapRef<4, 4>,
    #[prog("tracepoint/syscalls/sys_enter_execve")]
    pub execve: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_execveat")]
    pub execveat: ebpf::ProgRef,
    // store/load context parameters
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
    #[prog("tracepoint/syscalls/sys_enter_accept4")]
    pub enter_accept4: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_exit_accept4")]
    pub exit_accept4: ebpf::ProgRef,
    #[prog("tracepoint/syscalls/sys_enter_close")]
    pub enter_close: ebpf::ProgRef,
    #[hashmap(size = 0x2000)]
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
    fn check_pid(&self) -> Result<u64, i32> {
        use ebpf::helpers;

        let x = unsafe { helpers::get_current_pid_tgid() };
        let pid = (x >> 32) as u32;

        if let Some(&flags) = self.pid.get(&pid.to_ne_bytes()) {
            let flags = u32::from_ne_bytes(flags);

            if flags == 0xffffffff {
                return Ok(x);
            }
        }

        Err(0)
    }

    #[allow(clippy::nonminimal_bool)]
    #[inline(never)]
    fn check_env_entry(&mut self, entry: *const u8) -> Result<(), i32> {
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
            Ok(())
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

            if let Ok(()) = self.check_env_entry(entry) {
                env_str.discard();
                let x = unsafe { helpers::get_current_pid_tgid() };
                let pid = (x >> 32) as u32;
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

    #[inline(always)]
    pub fn execve(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        let env = ctx.read_here::<*const *const u8>(0x20);
        self.check_env_flag(env)
    }

    #[inline(always)]
    pub fn execveat(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        let env = ctx.read_here::<*const *const u8>(0x28);
        self.check_env_flag(env)
    }

    #[inline(always)]
    fn enter(&mut self, data: context::Variant) -> Result<(), i32> {
        use core::{mem, ptr};
        use ebpf::helpers;

        let (_, thread_id) = {
            let x = self.check_pid()?;
            ((x >> 32) as u32, (x & 0xffffffff) as u32)
        };
        let ts = unsafe { helpers::ktime_get_ns() };

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

        let (_, thread_id) = {
            let x = self.check_pid()?;
            ((x >> 32) as u32, (x & 0xffffffff) as u32)
        };
        let ts1 = unsafe { helpers::ktime_get_ns() };

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
        let x = unsafe { ebpf::helpers::get_current_pid_tgid() };
        let pid = (x >> 32) as u32;

        let event = Event::new(pid, ts0, ts1);
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
            context::Variant::Accept {
                listen_on_fd,
                addr_len,
                ..
            } => {
                let _ = listen_on_fd;
                let fd = ret as _;
                let event = event.set_tag_fd(DataTag::Accept, fd);
                if ret < 0 {
                    event.set_err(ret)
                } else {
                    let socket_id = ((fd as u64) << 32) + (pid as u64);
                    self.connections
                        .insert(socket_id.to_ne_bytes(), 0x1_u32.to_ne_bytes())?;
                    event.set_ok(addr_len)
                }
            }
            context::Variant::Send { fd, .. } | context::Variant::Write { fd, .. } => {
                let event = event.set_tag_fd(DataTag::Write, fd);
                let socket_id = ((fd as u64) << 32) + (pid as u64);
                if self.connections.get(&socket_id.to_ne_bytes()).is_none() {
                    return Ok(());
                }
                if ret < 0 {
                    event.set_err(ret)
                } else {
                    event.set_ok(ret as _)
                }
            }
            context::Variant::Recv { fd, .. } | context::Variant::Read { fd, .. } => {
                let event = event.set_tag_fd(DataTag::Read, fd);
                let socket_id = ((fd as u64) << 32) + (pid as u64);
                if self.connections.get(&socket_id.to_ne_bytes()).is_none() {
                    return Ok(());
                }
                if ret < 0 {
                    event.set_err(ret)
                } else {
                    event.set_ok(ret as _)
                }
            }
            _ => return Ok(()),
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
    pub fn enter_accept4(&mut self, ctx: ebpf::Context) -> Result<(), i32> {
        self.enter(context::Variant::Accept {
            listen_on_fd: ctx.read_here::<u64>(0x10) as u32,
            addr_ptr: ctx.read_here::<u64>(0x18),
            addr_len: ctx.read_here::<u64>(0x20),
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

        let _ = self.check_pid()?;

        let fd = ctx.read_here::<u64>(0x10) as u32;
        let (pid, _) = {
            let x = unsafe { helpers::get_current_pid_tgid() };
            ((x >> 32) as u32, (x & 0xffffffff) as u32)
        };
        let ts = unsafe { helpers::ktime_get_ns() };

        let socket_id = ((fd as u64) << 32) + (pid as u64);
        if self.connections.remove(&socket_id.to_ne_bytes())?.is_none() {
            return Ok(());
        }

        let event = Event::new(pid, ts, ts);
        let event = event.set_tag_fd(DataTag::Close, fd);
        send::dyn_sized::<typenum::B0>(&mut self.event_queue, event, ptr::null())
    }

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
}

#[cfg(feature = "user")]
fn main() {
    use bpf_recorder::sniffer_event::{SnifferEvent, SnifferEventVariant};
    use ebpf::{Skeleton, kind::AppItem};
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };
    use bpf_ring_buffer::RingBuffer;

    let env = env_logger::Env::default().default_filter_or("info");
    env_logger::init_from_env(env);
    sudo::escalate_if_needed().expect("failed to obtain superuser permission");
    let terminating = Arc::new(AtomicBool::new(false));
    {
        let terminating = terminating.clone();
        ctrlc::set_handler(move || {
            terminating.store(true, Ordering::Relaxed);
        })
        .expect("Error setting Ctrl-C handler");
    }

    static CODE: &[u8] = include_bytes!(concat!("../", env!("BPF_CODE_RECORDER")));

    let mut skeleton = Skeleton::<App>::open("bpf-recorder\0", CODE)
        .unwrap_or_else(|code| panic!("failed to open bpf: {}", code));
    skeleton
        .load()
        .unwrap_or_else(|code| panic!("failed to load bpf: {}", code));
    let (skeleton,mut app) = skeleton
        .attach()
        .unwrap_or_else(|code| panic!("failed to attach bpf: {}", code));
    log::info!("attached bpf module");

    let fd = match app.event_queue.kind_mut() {
        ebpf::kind::AppItemKindMut::Map(map) => map.fd(),
        _ => unreachable!(),
    };
    let mut rb = RingBuffer::new(fd, 0x8000000).unwrap();
    while !terminating.load(Ordering::SeqCst) {
        let events = rb.read_blocking::<SnifferEvent>(&terminating).unwrap();
        for event in events {
            match &event.variant {
                SnifferEventVariant::Error(_, _) => (),
                variant => {
                    log::info!("{event}");
                    log::info!("{variant}")
                },
            }
        }
    }
    log::info!("terminated");
    drop(skeleton);
}
