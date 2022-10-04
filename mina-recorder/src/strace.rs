use std::{time::Duration, sync::mpsc, process::Child};

use radiation::{Absorb, Emit};
use serde::Serialize;
use strace_parse::{raw, structure};

use crate::{database::DbStrace, custom_coding};

#[derive(Absorb, Emit, Serialize)]
pub struct StraceLine {
    pub call: String,
    pub args: Vec<String>,
    pub result: Option<String>,
    #[custom_absorb(custom_coding::duration_absorb)]
    #[custom_emit(custom_coding::duration_emit)]
    pub start: Duration,
}

pub fn process(mut source: Child, db: DbStrace, rx: mpsc::Receiver<()>) {
    let read = source.stdout.as_mut().unwrap();
    let it = structure::iter_finished(raw::parse(read));
    for x in it {
        if rx.try_recv().is_ok() {
            break;
        }
        let syscall = match x {
            Err(err) => {
                log::error!("uhnable to parse strace output: {err}");
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
                if let Err(err) = db.add_strace_line(StraceLine { call, args, result, start }) {
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
