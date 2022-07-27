mod recorder;
pub use self::recorder::P2pRecorder;

mod connection;

use std::{
    net::SocketAddr,
    fmt,
    time::{SystemTime, Duration},
};

#[derive(Debug, Clone)]
pub struct EventMetadata {
    pub id: ConnectionId,
    pub time: SystemTime,
    pub duration: Duration,
}

impl EventMetadata {
    #[cfg(test)]
    pub fn fake() -> Self {
        EventMetadata {
            id: ConnectionId {
                alias: "fake".to_string(),
                addr: "127.0.0.1:0".parse().unwrap(),
                pid: 0,
                fd: 0,
            },
            time: SystemTime::now(),
            duration: Duration::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId {
    pub alias: String,
    pub addr: SocketAddr,
    pub pid: u32,
    pub fd: u32,
}

#[derive(Clone)]
pub struct DirectedId {
    pub metadata: EventMetadata,
    pub incoming: bool,
}

impl fmt::Display for DirectedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use time::OffsetDateTime;

        let EventMetadata { id, time, duration } = &self.metadata;
        let (hour, minute, second, nano) = OffsetDateTime::from(*time).time().as_hms_nano();
        let ConnectionId {
            alias,
            pid,
            addr,
            fd,
        } = id;

        let arrow = if self.incoming { "->" } else { "<-" };

        write!(f, "{hour:02}:{minute:02}:{second:02}:{nano:09} {duration:010?} {addr} {fd} {arrow} {alias}_{pid}")
    }
}
