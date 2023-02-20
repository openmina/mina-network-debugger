use std::{
    process::{Command, Child, Stdio},
    fs,
    net::{SocketAddr, IpAddr},
    time::{SystemTime, Duration},
    thread::{self, JoinHandle},
    io::Read,
};

use crate::message::NetReport;

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
                .filter_map(|x| x.children().find(|x| x.tag_name().name() == "tcpflow"))
                .filter_map(|x| {
                    use time::format_description::well_known::Iso8601;
                    use time::PrimitiveDateTime;

                    let a = || x.attributes();
                    let start_time = a().find(|x| x.name() == "startime")?.value();
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

                    let src = SocketAddr::new(src_ip, srcport);
                    let dst = SocketAddr::new(dst_ip, dstport);

                    let date = PrimitiveDateTime::parse(start_time, &Iso8601::DEFAULT).ok()?;

                    Some(NetReport {
                        local: if self.this_ip == src_ip { src } else { dst },
                        remote: if self.this_ip == src_ip { dst } else { src },
                        timestamp: SystemTime::from(date.assume_utc()),
                    })
                })
                .collect::<Vec<_>>(),
            Err(err) => {
                log::error!("cannot parse xml report: {err}");
                vec![]
            }
        };

        if cns.is_empty() {
            for entry in fs::read_dir("/test").ok()?.filter_map(Result::ok) {
                let name = entry.file_name();
                let name = name.as_os_str().to_str().unwrap();
                if name.len() == 54 {
                    let mut pair = name.split('T');
                    let timestamp = pair.next()?;
                    let pair = pair.next()?;

                    let srcport = pair[16..21].parse::<u16>().ok()?;
                    let dstport = pair[38..43].parse::<u16>().ok()?;

                    let src_ip = parse_ip(&pair[..15])?;
                    let dst_ip = parse_ip(&pair[22..37])?;

                    let src = SocketAddr::new(src_ip, srcport);
                    let dst = SocketAddr::new(dst_ip, dstport);

                    let timestamp = SystemTime::UNIX_EPOCH
                        .checked_add(Duration::from_secs(timestamp.parse::<u64>().ok()?))?;

                    cns.push(NetReport {
                        local: if self.this_ip == src_ip { src } else { dst },
                        remote: if self.this_ip == src_ip { dst } else { src },
                        timestamp,
                    });
                }
            }
        }

        cns.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        Some(cns)
    }
}

fn parse_ip(s: &str) -> Option<IpAddr> {
    let mut octets = s.split('.');
    let a = octets.next()?.parse().ok()?;
    let b = octets.next()?.parse().ok()?;
    let c = octets.next()?.parse().ok()?;
    let d = octets.next()?.parse().ok()?;

    Some(IpAddr::V4(std::net::Ipv4Addr::new(a, b, c, d)))
}
