mod recorder;
pub use self::recorder::P2pRecorder;

mod connection;

mod custom_coding;

use std::{
    net::SocketAddr,
    fmt,
    time::{SystemTime, Duration},
};

use radiation::{Emit, Absorb};

#[derive(Debug, Clone, Emit, Absorb)]
pub struct EventMetadata {
    pub id: ConnectionInfo,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub time: SystemTime,
    #[custom_absorb(custom_coding::duration_absorb)]
    #[custom_emit(custom_coding::duration_emit)]
    pub duration: Duration,
}

#[derive(Debug, Clone, Emit, Absorb, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionInfo {
    #[custom_absorb(custom_coding::addr_absorb)]
    #[custom_emit(custom_coding::addr_emit)]
    pub addr: SocketAddr,
    pub pid: u32,
    pub fd: u32,
}

#[derive(Clone)]
pub struct DirectedId {
    pub metadata: EventMetadata,
    pub alias: String,
    pub incoming: bool,
}

impl fmt::Display for DirectedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use time::OffsetDateTime;

        let EventMetadata { id, time, duration } = &self.metadata;
        let (hour, minute, second, nano) = OffsetDateTime::from(*time).time().as_hms_nano();
        let ConnectionInfo { pid, addr, fd } = id;

        let arrow = if self.incoming { "->" } else { "<-" };
        let alias = &self.alias;

        write!(f, "{hour:02}:{minute:02}:{second:02}:{nano:09} {duration:010?} {addr} {fd} {arrow} {alias}_{pid}")
    }
}
