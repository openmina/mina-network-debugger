use std::{time::Duration, sync::mpsc, process::Child};

use radiation::{Absorb, Emit};
use serde::Serialize;
use strace_parse::{raw, structure};

use crate::{database::DbStrace, custom_coding};

#[derive(Absorb, Emit, Serialize)]
pub struct StraceLine {
    pub call: String,
    pub pid: u32,
    pub args: Vec<String>,
    pub result: Option<String>,
    #[custom_absorb(custom_coding::duration_absorb)]
    #[custom_emit(custom_coding::duration_emit)]
    pub start: Duration,
}

pub fn process(mut source: Child, db: DbStrace, rx: mpsc::Receiver<()>) {
    let read = source.stderr.as_mut().unwrap();
    let it = structure::iter_finished(raw::parse(read));
    for x in it {
        if rx.try_recv().is_ok() {
            break;
        }
        let syscall = match x {
            Err(err) => {
                log::debug!("{err}");
                continue;
            }
            Ok(v) => v,
        };
        match syscall.call {
            raw::Call::Generic(raw::GenericCall { call, args, result }) => {
                let result = match result {
                    raw::CallResult::Value(s) => Some(s),
                    raw::CallResult::Unknown => None,
                };
                let start = syscall.start.unwrap_or_default();
                let pid = syscall.pid.unwrap_or(u32::MAX);
                let line = StraceLine {
                    pid,
                    call,
                    args,
                    result,
                    start,
                };
                if let Err(err) = db.add_strace_line(line) {
                    log::error!("database error when writing strace {err}");
                }
            }
            raw::Call::Unfinished(_) | raw::Call::Resumed(_) => {
                log::error!("{:?}, must not happen", syscall.call);
            }
            _ => (),
        }
    }
    source.kill().expect("must kill strace");
}
