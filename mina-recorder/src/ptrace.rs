use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    any::Any,
    collections::BTreeMap,
    mem,
};

use pete::{Error, Ptracer, Pid, Stop, Restart};

pub struct Task {
    running_state: bool,
    running: Arc<AtomicBool>,
    terminating: Arc<AtomicBool>,
    task: Option<thread::JoinHandle<Result<(), Error>>>,
}

impl Task {
    pub fn spawn(pid: u32) -> Self {
        let running_state = false;
        let running = Arc::new(AtomicBool::new(true));
        let terminating = Arc::new(AtomicBool::new(false));

        let mut tracer = Ptracer::new();
        let pid = Pid::from_raw(pid as i32);

        let handle = {
            let running = running.clone();
            let terminating = terminating.clone();

            thread::spawn(move || -> Result<(), Error> {
                tracer.attach(pid)?;

                let mut suspended = BTreeMap::new();

                while let Some(tracee) = tracer.wait()? {
                    if tracee.pid == pid && matches!(&tracee.stop, Stop::Exiting { .. }) {
                        break;
                    }
                    if terminating.load(Ordering::SeqCst) {
                        break;
                    }
                    if running.load(Ordering::SeqCst) {
                        for suspended_tracee in mem::take(&mut suspended).into_values() {
                            tracer.restart(suspended_tracee, Restart::Syscall)?;
                        }
                    }
                    if matches!(&tracee.stop, Stop::SyscallExit) {
                        let regs = tracee.registers()?;
                        if matches!(regs.orig_rax, 215 | 232 | 281) {
                            suspended.insert(tracee.pid, tracee);
                            continue;
                        }
                    }
                    tracer.restart(tracee, Restart::Syscall)?;
                }

                Ok(())
            })
        };
        let task = Some(handle);

        Task {
            running_state,
            running,
            terminating,
            task,
        }
    }

    pub fn set_running(&mut self, running: bool) {
        if self.running_state != running {
            if running {
                log::debug!("resume");
            } else {
                log::debug!("suspend");
            }
            self.running.store(false, Ordering::SeqCst);
            self.running_state = running;
        }
    }

    pub fn is_running(&self) -> bool {
        self.running_state
    }

    pub fn join(mut self) -> Result<(), Box<dyn Any + Send + 'static>> {
        self.terminating.store(true, Ordering::SeqCst);
        if let Some(handle) = self.task.take() {
            handle
                .join()
                .and_then(|r| r.map_err(Box::new).map_err(|e| e as _))
        } else {
            Ok(())
        }
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
