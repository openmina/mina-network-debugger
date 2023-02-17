use std::{
    net::{IpAddr, SocketAddr},
    process::Child,
    str::FromStr,
    thread,
};

use crate::message::DebuggerReport;

pub struct Conntrack {
    child: Child,
    thread: thread::JoinHandle<DebuggerReport>,
}

pub fn run() -> anyhow::Result<Conntrack> {
    use std::{env, time::SystemTime};

    use super::message::ConnectionMetadata;

    let this = env::var("MY_POD_IP")?.parse::<IpAddr>()?;
    let (child, iter) = launch();

    let thread = thread::spawn(move || DebuggerReport {
        version: String::new(),
        ipc: Default::default(),
        network: {
            iter.filter(|msg| msg.state.tcp_established())
                .map(|msg| {
                    let peer_addr = if msg.dst.ip() == this {
                        msg.src.ip()
                    } else {
                        msg.dst.ip()
                    };
                    (
                        peer_addr,
                        ConnectionMetadata {
                            incoming: msg.dst.ip() == this,
                            fd: -1,
                            checksum: Default::default(),
                            timestamp: SystemTime::now(),
                        },
                    )
                })
                .collect()
        },
    });

    Ok(Conntrack { child, thread })
}

impl Conntrack {
    pub fn stop(mut self) -> DebuggerReport {
        self.child.kill().unwrap();
        self.thread.join().unwrap()
    }
}

pub fn launch() -> (std::process::Child, impl Iterator<Item = Msg>) {
    use std::{
        io::{self, BufRead},
        process::{Command, Stdio},
    };

    let mut conntrack = Command::new("conntrack")
        .args(&["-E", "--protonum=tcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    let iter = io::BufReader::new(conntrack.stdout.take().unwrap())
        .lines()
        .filter_map(Result::ok)
        .inspect(|line| log::info!("{line}"))
        .filter_map(|line| line.parse::<Msg>().ok());
    (conntrack, iter)
}

pub struct Msg {
    pub event: Event,
    pub state: ProtocolState,
    pub src: SocketAddr,
    pub dst: SocketAddr,
    pub status: Option<Status>,
    pub expected_src: SocketAddr,
    pub expected_dst: SocketAddr,
    pub reply_status: Option<Status>,
}

impl FromStr for Msg {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut words = s.split_whitespace();
        let event = words.next().ok_or(())?.parse().map_err(drop)?;
        let "tcp" = words.next().ok_or(())? else {
            return Err(());
        };
        let 6 = words.next().ok_or(())?.parse().map_err(drop)? else {
            return Err(());
        };
        let ttl = words.next().ok_or(())?.parse().map_err(drop)?;
        let state = words.next().ok_or(())?.parse().map_err(drop)?;
        let src = words
            .next()
            .ok_or(())?
            .trim_start_matches("src=")
            .parse()
            .map_err(drop)?;
        let dst = words
            .next()
            .ok_or(())?
            .trim_start_matches("dst=")
            .parse()
            .map_err(drop)?;
        let sport = words
            .next()
            .ok_or(())?
            .trim_start_matches("sport=")
            .parse()
            .map_err(drop)?;
        let dport = words
            .next()
            .ok_or(())?
            .trim_start_matches("dport=")
            .parse()
            .map_err(drop)?;

        let word = words.next().ok_or(())?;
        let (status, next) = if word.starts_with('[') {
            (Some(word.parse().map_err(drop)?), words.next().ok_or(())?)
        } else {
            (None, word)
        };
        let e_src = next.trim_start_matches("src=").parse().map_err(drop)?;
        let e_dst = words
            .next()
            .ok_or(())?
            .trim_start_matches("dst=")
            .parse()
            .map_err(drop)?;
        let e_sport = words
            .next()
            .ok_or(())?
            .trim_start_matches("sport=")
            .parse()
            .map_err(drop)?;
        let e_dport = words
            .next()
            .ok_or(())?
            .trim_start_matches("dport=")
            .parse()
            .map_err(drop)?;
        let reply_status = words
            .next()
            .map(Status::from_str)
            .transpose()
            .map_err(drop)?;

        Ok(Msg {
            event,
            state: ProtocolState::Tcp { ttl, state },
            src: SocketAddr::new(src, sport),
            dst: SocketAddr::new(dst, dport),
            status,
            expected_src: SocketAddr::new(e_src, e_sport),
            expected_dst: SocketAddr::new(e_dst, e_dport),
            reply_status,
        })
    }
}

pub enum Event {
    New,
    Destroy,
    Update,
}

impl FromStr for Event {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "[NEW]" => Ok(Self::New),
            "[DESTROY]" => Ok(Self::Destroy),
            "[UPDATE]" => Ok(Self::Update),
            unknown => Err(unknown.to_owned()),
        }
    }
}

pub enum ProtocolState {
    Tcp { ttl: u32, state: TcpState },
}

impl ProtocolState {
    pub fn tcp_established(&self) -> bool {
        matches!(
            self,
            Self::Tcp {
                state: TcpState::Established,
                ..
            }
        )
    }
}

pub enum TcpState {
    SynSent,
    SynRecv,
    Established,
    Close,
    CloseWait,
    TimeWait,
    FinWait,
    LastAck,
}

impl FromStr for TcpState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SYN_SENT" => Ok(Self::SynSent),
            "SYN_RECV" => Ok(Self::SynRecv),
            "ESTABLISHED" => Ok(Self::Established),
            "CLOSE" => Ok(Self::Close),
            "CLOSE_WAIT" => Ok(Self::CloseWait),
            "TIME_WAIT" => Ok(Self::TimeWait),
            "FIN_WAIT" => Ok(Self::FinWait),
            "LAST_ACK" => Ok(Self::LastAck),
            unknown => Err(unknown.to_owned()),
        }
    }
}

pub enum Status {
    Assured,
    Unreplied,
}

impl FromStr for Status {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "[ASSURED]" => Ok(Self::Assured),
            "[UNREPLIED]" => Ok(Self::Unreplied),
            unknown => Err(unknown.to_owned()),
        }
    }
}
