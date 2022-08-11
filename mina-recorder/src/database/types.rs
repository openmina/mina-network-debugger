use std::{time::SystemTime, fmt};

use radiation::{Absorb, Emit};

use serde::{Serialize};

use crate::{ConnectionInfo, custom_coding};

#[derive(Clone, Copy, Debug, Absorb, Emit, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId(pub u64);

#[derive(Clone, Absorb, Emit, Serialize)]
pub struct Connection {
    pub info: ConnectionInfo,
    pub incoming: bool,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub timestamp: SystemTime,
}

/// Positive ids are streams from initiator, negatives are from responder
#[derive(Clone, Copy, Debug, Absorb, Emit, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamId(pub u64);

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let StreamId(id) = self;
        write!(f, "stream{id:08x}")
    }
}

#[repr(u16)]
#[derive(Clone, Copy, Serialize)]
#[serde(rename = "snake_case")]
pub enum StreamKind {
    Unknown = 0xffff,
    Raw = 0x0000,
    Handshake = 0x0001,
    Kad = 0x0100,
    IpfsId = 0x0200,
    IpfsPush = 0x0201,
    PeerExchange = 0x0300,
    Meshsub = 0x0400,
    Rpc = 0x0500,
}

impl StreamKind {
    pub fn iter() -> impl Iterator<Item = Self> {
        [
            StreamKind::Raw,
            StreamKind::Handshake,
            StreamKind::Kad,
            StreamKind::IpfsId,
            StreamKind::IpfsPush,
            StreamKind::PeerExchange,
            StreamKind::Meshsub,
            StreamKind::Rpc,
        ]
        .into_iter()
    }
}

#[derive(Clone, Serialize)]
#[serde(rename = "snake_case")]
pub enum StreamMeta {
    Raw,
    Handshake,
    Forward(u64),
    Backward(u64),
}

#[derive(Clone, Absorb, Emit, Serialize)]
pub struct Stream {
    pub connection_id: ConnectionId,
    #[custom_absorb(custom_coding::stream_meta_absorb)]
    #[custom_emit(custom_coding::stream_meta_emit)]
    pub meta: StreamMeta,
    #[custom_absorb(custom_coding::stream_kind_absorb)]
    #[custom_emit(custom_coding::stream_kind_emit)]
    pub kind: StreamKind,
}

#[derive(Clone, Copy, Debug, Absorb, Emit, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MessageId(pub u64);

#[derive(Clone, Absorb, Serialize, Emit)]
pub struct Message {
    pub stream_id: StreamId,
    pub incoming: bool,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub timestamp: SystemTime,
    pub offset: u64,
    pub size: u32,
}
