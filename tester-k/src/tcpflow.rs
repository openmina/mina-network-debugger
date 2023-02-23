use std::{
    process::{Command, Child, Stdio},
    fs,
    net::{SocketAddr, IpAddr},
    time::SystemTime,
    thread::{self, JoinHandle},
    io::Read, collections::BTreeMap,
};

use crate::{message::NetReport, constants};

pub struct TcpFlow {
    child: Child,
    thread: JoinHandle<String>,
    this_ip: IpAddr,
}

impl TcpFlow {
    pub fn run(this_ip: IpAddr) -> anyhow::Result<Self> {
        fs::remove_dir_all("/test").unwrap_or_default();
        fs::create_dir_all("/test")?;

        let mut child = Command::new("tcpflow")
            .env("TZ", "UTC")
            .current_dir("/test")
            .arg("-Ft")
            .arg("-X/proc/self/fd/1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let mut stdout = child.stdout.take().unwrap();
        let thread = thread::spawn(move || {
            let mut s = String::new();
            stdout.read_to_string(&mut s).unwrap();
            s
        });

        Ok(TcpFlow {
            child,
            thread,
            this_ip,
        })
    }

    pub fn stop(mut self) -> Option<Vec<NetReport>> {
        use nix::{
            sys::signal::{self, Signal},
            unistd::Pid,
        };

        signal::kill(Pid::from_raw(self.child.id() as i32), Signal::SIGINT)
            .expect("cannot send ctrl-c to tcpflow subprocess");
        self.child.wait().expect("cannot wait tcpflow subprocess");

        let report = self.thread.join().unwrap();
        let mut cns = match roxmltree::Document::parse(&report) {
            Ok(doc) => doc
                .root_element()
                .children()
                .filter(|x| x.tag_name().name() == "configuration")
                .map(|x| x.children())
                .flatten()
                .filter(|x| {
                    let size = x.children()
                        .find(|x| x.tag_name().name() == "filesize")
                        .and_then(|a| a.text().and_then(|text| text.parse::<usize>().ok()))
                        .unwrap_or_default();
                    size > 0
                })
                .filter_map(|x| {
                    let filesize = x.children().find(|x| x.tag_name().name() == "filesize")?;
                    let tcpflow = x.children().find(|x| x.tag_name().name() == "tcpflow")?;
                    Some((filesize, tcpflow))
                })
                .filter_map(|(filesize, x)| {
                    use time::format_description::well_known::Iso8601;
                    use time::PrimitiveDateTime;

                    let a = || x.attributes();
                    let start_time = a().find(|x| x.name() == "startime")?.value();
                    let date = PrimitiveDateTime::parse(start_time, &Iso8601::DEFAULT).ok()?;

                    let bytes_number = filesize.text()?.parse().ok()?;

                    let src_ip = a()
                        .find(|x| x.name() == "src_ipn")?
                        .value()
                        .parse::<IpAddr>()
                        .ok()?;

                    let srcport = a()
                        .find(|x| x.name() == "srcport")?
                        .value()
                        .parse::<u16>()
                        .ok()?;
                    if srcport == constants::CENTER_PORT {
                        // skip connection to itself
                        return None;
                    }

                    let dst_ip = a()
                        .find(|x| x.name() == "dst_ipn")?
                        .value()
                        .parse::<IpAddr>()
                        .ok()?;
                    let dstport = a()
                        .find(|x| x.name() == "dstport")?
                        .value()
                        .parse::<u16>()
                        .ok()?;
                    if dstport == constants::CENTER_PORT {
                        // skip connection to itself
                        return None;
                    }

                    let src = SocketAddr::new(src_ip, srcport);
                    let dst = SocketAddr::new(dst_ip, dstport);

                    Some(NetReport {
                        local: src,
                        remote: dst,
                        counter: 0,
                        timestamp: SystemTime::from(date.assume_utc()),
                        bytes_number,
                    })
                })
                .collect::<Vec<_>>(),
            Err(err) => {
                log::error!("cannot parse xml report: {err}");
                vec![]
            }
        };

        let mut lengths = cns.iter().filter_map(|cn| {
            if self.this_ip == cn.local.ip() {
                None
            } else {
                Some((cn.local.ip(), cn.bytes_number))
            }
        }).collect::<BTreeMap<_, _>>();
        // tcpflow counts each connection twice (from local to remote and vice versa)
        // let's ignore half of them
        cns.retain_mut(|cn| {
            cn.bytes_number += lengths.remove(&cn.remote.ip()).unwrap_or_default();
            self.this_ip == cn.local.ip()
        });

        cns.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        let mut counters = BTreeMap::new();
        for item in &mut cns {
            let counter = counters.entry(item.remote).or_default();
            item.counter = *counter;
            *counter += 1;
        }

        Some(cns)
    }
}
