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
    pub duration: Duration,
}

#[derive(Debug, Clone, Emit, Absorb, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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

impl DirectedId {
    #[cfg(test)]
    pub fn fake() -> Self {
        DirectedId {
            metadata: EventMetadata {
                id: ConnectionInfo {
                    addr: "127.0.0.1:0".parse().expect("valid constant"),
                    pid: 1,
                    fd: 1,
                },
                time: SystemTime::now(),
                duration: Duration::from_secs(1),
            },
            alias: String::default(),
            incoming: true,
        }
    }
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

#[derive(Absorb, Emit)]
pub struct ChunkHeader {
    pub size: u32,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub time: SystemTime,
    pub encryption_status: EncryptionStatus,
    pub incoming: bool,
}

#[derive(Clone, Debug, Absorb, Emit)]
#[tag(u8)]
pub enum EncryptionStatus {
    #[tag(1)]
    Raw,
    #[tag(0xff)]
    DecryptedPnet,
    #[tag(0)]
    DecryptedNoise,
}

impl ChunkHeader {
    pub const SIZE: usize = 18; // size 4 + time 12 + encrypted 1 + incoming 1
}

impl fmt::Display for ChunkHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use time::OffsetDateTime;

        let (hour, minute, second, nano) = OffsetDateTime::from(self.time).time().as_hms_nano();
        let incoming = if self.incoming {
            "incoming"
        } else {
            "outgoing"
        };
        let status = &self.encryption_status;

        write!(
            f,
            "{hour:02}:{minute:02}:{second:02}.{nano:09} {status:?} {incoming}"
        )
    }
}
