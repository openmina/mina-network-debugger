use std::{
    net::SocketAddr,
    fmt,
    time::{SystemTime, Duration},
};

use radiation::{Emit, Absorb};

use serde::{Serialize, Deserialize};

use crate::custom_coding;

#[derive(Debug, Clone)]
pub struct EventMetadata {
    pub id: ConnectionInfo,
    pub time: SystemTime,
    pub better_time: SystemTime,
    pub duration: Duration,
}

impl Default for EventMetadata {
    fn default() -> Self {
        EventMetadata {
            id: ConnectionInfo::default(),
            time: SystemTime::UNIX_EPOCH,
            better_time: SystemTime::UNIX_EPOCH,
            duration: Duration::from_secs(0),
        }
    }
}

#[derive(Debug, Clone, Emit, Absorb, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionInfo {
    #[custom_absorb(custom_coding::addr_absorb)]
    #[custom_emit(custom_coding::addr_emit)]
    pub addr: SocketAddr,
    pub pid: u32,
    pub fd: u32,
}

impl Default for ConnectionInfo {
    fn default() -> Self {
        ConnectionInfo {
            addr: "127.0.0.1:0".parse().expect("valid constant"),
            pid: 1,
            fd: 1,
        }
    }
}

#[derive(Clone)]
pub struct DirectedId {
    pub metadata: EventMetadata,
    pub alias: String,
    pub incoming: bool,
    pub buffered: usize,
}

impl Default for DirectedId {
    fn default() -> Self {
        DirectedId {
            metadata: EventMetadata::default(),
            alias: String::default(),
            incoming: true,
            buffered: 0,
        }
    }
}

impl fmt::Display for EventMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use time::OffsetDateTime;

        let EventMetadata { id, time, duration, .. } = self;
        let (hour, minute, second, nano) = OffsetDateTime::from(*time).time().as_hms_nano();
        let ConnectionInfo { pid, addr, fd } = id;

        write!(
            f,
            "{hour:02}:{minute:02}:{second:02}.{nano:09} {duration:010?} {addr} {fd} pid: {pid}"
        )
    }
}

impl fmt::Display for DirectedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use time::OffsetDateTime;

        let EventMetadata { id, time, duration, .. } = &self.metadata;
        let (hour, minute, second, nano) = OffsetDateTime::from(*time).time().as_hms_nano();
        let ConnectionInfo { pid, addr, fd } = id;

        let arrow = if self.incoming { "->" } else { "<-" };
        let alias = &self.alias;
        let buffered = self.buffered;

        write!(f, "{hour:02}:{minute:02}:{second:02}.{nano:09} {duration:010?} {buffered} {addr} {fd} {arrow} {alias}_{pid}")
    }
}
