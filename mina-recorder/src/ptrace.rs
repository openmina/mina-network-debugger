use std::{
    io::{Write, self},
    thread,
    collections::{BTreeMap, BTreeSet},
    mem,
    process::{Command, Stdio},
    sync::mpsc,
};

use pete::{Error, Ptracer, Pid, Stop, Restart, Signal};

pub struct Task {
    running_state: bool,
    task: Option<thread::JoinHandle<Result<(), Error>>>,
    sender: mpsc::Sender<Act>,
}

#[derive(Debug)]
pub enum Act {
    Terminate,
    Suspend,
    Resume,
    Attach(Pid),
}

impl From<i32> for Act {
    fn from(v: i32) -> Self {
        match v {
            0 => Act::Terminate,
            -1 => Act::Suspend,
            1 => Act::Resume,
            p => Act::Attach(Pid::from_raw(p)),
        }
    }
}

impl From<Act> for i32 {
    fn from(v: Act) -> Self {
        match v {
            Act::Terminate => 0,
            Act::Suspend => -1,
            Act::Resume => 1,
            Act::Attach(pid) => pid.as_raw(),
        }
    }
}

impl Task {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let tracer = Ptracer::new();

        let (sender, receiver) = mpsc::channel();
        let task = thread::spawn(move || Self::run_tracer(tracer, receiver));

        Task {
            running_state: true,
            task: Some(task),
            sender,
        }
    }

    fn run_tracer(mut tracer: Ptracer, receiver: mpsc::Receiver<Act>) -> Result<(), Error> {
        let mut channel = Command::new("cat");
        channel.arg("-").stdin(Stdio::piped()).stdout(Stdio::null());
        let mut channel = tracer.spawn(channel)?;

        let channel_pid = Pid::from_raw(channel.id() as _);
        let mut writer = channel
            .stdin
            .take()
            .expect("`cat` process must have piped stdin");
        let channel_thread = thread::spawn(move || -> io::Result<()> {
            while let Ok(act) = receiver.recv() {
                let terminate = matches!(&act, Act::Terminate);
                let v = i32::from(act).to_ne_bytes();
                writer.write_all(&v)?;
                if terminate {
                    break;
                }
            }

            Ok(())
        });

        let mut running = true;
        let mut suspended = BTreeMap::new();
        let mut tracees = BTreeSet::new();

        'main: while let Some(mut tracee) = tracer.wait()? {
            if tracee.pid == channel_pid && matches!(&tracee.stop, Stop::SyscallExit) {
                let regs = tracee.registers()?;
                // `regs.orig_rax == 0` - `sys_read` syscall
                // `regs.rdi == 0` - `fd` is stdin
                // `regs.rax as i64 > 0` - the reading yield data
                if regs.orig_rax == 0 && regs.rdi == 0 && regs.rax as i64 > 0 {
                    let bytes = tracee.read_memory(regs.rsi, regs.rax as usize)?;
                    for chunk in bytes.chunks(4) {
                        if chunk.len() != 4 {
                            log::warn!("must write in `cat` process only 4 bytes pieces of data");
                            continue;
                        }
                        let act_i = i32::from_ne_bytes(chunk.try_into().expect("cannot fail"));
                        let act = Act::from(act_i);
                        match act {
                            Act::Terminate => break 'main,
                            Act::Resume => {
                                log::info!("received command {act:?}");
                                running = true;
                                for suspended_tracee in mem::take(&mut suspended).into_values() {
                                    tracer.restart(suspended_tracee, Restart::Syscall)?;
                                    log::info!("resume {}", suspended_tracee.pid);
                                }
                            }
                            Act::Suspend => {
                                log::info!("received command {act:?}");
                                running = false;
                            }
                            Act::Attach(pid) => {
                                if tracees.insert(pid) {
                                    log::info!("attach {pid}");
                                    tracer.attach(pid)?;
                                }
                            }
                        }
                    }
                }
            } else if tracee.pid != channel_pid && !running {
                // log::info!("want suspend {} {:?}", tracee.pid, tracee.stop);
                if matches!(
                    &tracee.stop,
                    Stop::SyscallExit
                        | Stop::SignalDelivery {
                            signal: Signal::SIGTRAP
                        }
                ) {
                    let regs = tracee.registers()?;
                    // log::info!("want suspend {} {}", tracee.pid, regs.orig_rax);
                    if matches!(regs.orig_rax, 215 | 232 | 281) {
                        log::info!("suspend {}", tracee.pid);
                        suspended.insert(tracee.pid, tracee);
                        // do not continue execution, the thread must remains stopped
                        // until resume command arrive
                        continue 'main;
                    }
                }
            } else if tracee.pid != channel_pid && matches!(&tracee.stop, Stop::Exiting { .. }) {
                tracees.remove(&tracee.pid);
            }
            tracer.restart(tracee, Restart::Syscall)?;
        }

        let _ = channel.kill();
        channel_thread.join().unwrap().unwrap();

        Ok(())
    }

    pub fn attach(&mut self, pid: u32) {
        self.sender
            .send(Act::Attach(Pid::from_raw(pid as i32)))
            .unwrap_or_default();
    }

    pub fn set_running(&mut self, running: bool) {
        if self.running_state != running {
            if running {
                self.sender.send(Act::Resume).unwrap_or_default();
            } else {
                self.sender.send(Act::Suspend).unwrap_or_default();
            }
            self.running_state = running;
        }
    }

    pub fn is_running(&self) -> bool {
        self.running_state
    }

    pub fn join(mut self) -> Result<(), Error> {
        self.sender.send(Act::Terminate).unwrap_or_default();
        if let Some(task) = self.task.take() {
            task.join().unwrap()?;
        }
        Ok(())
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        assert!(
            self.task.is_none(),
            "must join ptrace task using `Task::join`"
        );
    }
}
