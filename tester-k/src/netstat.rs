use std::{
    collections::BTreeSet,
    io::{self, BufRead},
    mem,
    net::SocketAddr,
    process::{Child, Command, Stdio},
    thread,
};

use crate::message::NetReport;

pub struct Netstat {
    child: Child,
    thread: thread::JoinHandle<Vec<NetReport>>,
}

impl Netstat {
    pub fn run() -> anyhow::Result<Self> {
        use std::time::SystemTime;

        let (child, iter) = launch();

        let thread = thread::spawn(move || {
            iter.map(|(local, remote)| NetReport {
                local,
                remote,
                timestamp: SystemTime::now(),
            })
            .collect()
        });

        Ok(Netstat { child, thread })
    }

    pub fn stop(mut self) -> Vec<NetReport> {
        self.child.kill().unwrap();
        self.thread.join().unwrap()
    }
}

pub fn launch() -> (Child, impl Iterator<Item = (SocketAddr, SocketAddr)>) {
    let mut process = Command::new("netstat")
        .arg("-ctn")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    let iter = io::BufReader::new(process.stdout.take().unwrap())
        .lines()
        .filter_map(Result::ok)
        .inspect(|line| log::info!("{line}"))
        .scan(BTreeSet::default(), |old, line| {
            let mut words = line.split_ascii_whitespace();
            if let Some("tcp") = words.next() {
                let _ = words.next()?;
                let _ = words.next()?;
                let local = words.next()?.parse::<SocketAddr>().ok()?;
                let remote = words.next()?.parse::<SocketAddr>().ok()?;
                if let "ESTABLISHED" = words.next()? {
                    old.insert((local, remote));
                }
                Some(BTreeSet::default())
            } else {
                Some(mem::replace(old, BTreeSet::default()))
            }
        })
        .filter(|group| !group.is_empty())
        .scan(BTreeSet::default(), |old, mut this| {
            let c = this.clone();
            for pair in old.iter() {
                this.remove(pair);
            }
            *old = c;
            Some(this)
        })
        .flatten();
    (process, iter)
}
