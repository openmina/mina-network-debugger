use std::{time::SystemTime, fmt, str::FromStr};

use radiation::{Absorb, Emit};

use serde::{Serialize};

use crate::{ConnectionInfo, custom_coding};

#[derive(Clone, Copy, Debug, Absorb, Emit, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId(pub u64);

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ConnectionId(id) = self;
        write!(f, "connection{id:08x}")
    }
}

#[derive(Clone, Absorb, Emit, Serialize)]
pub struct Connection {
    pub info: ConnectionInfo,
    pub incoming: bool,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub timestamp: SystemTime,
}

impl AsRef<SystemTime> for Connection {
    fn as_ref(&self) -> &SystemTime {
        &self.timestamp
    }
}

/// Positive ids are streams from initiator, negatives are from responder
#[derive(Clone, Copy, Debug, Absorb, Emit, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamFullId {
    pub cn: ConnectionId,
    #[custom_absorb(custom_coding::stream_meta_absorb)]
    #[custom_emit(custom_coding::stream_meta_emit)]
    pub id: StreamId,
}

impl fmt::Display for StreamFullId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let StreamFullId {
            cn: ConnectionId(cn),
            id,
        } = self;
        write!(f, "stream_{cn:08x}_{id}")
    }
}

#[repr(u16)]
#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Unknown = 0xffff,
    Handshake = 0x0001,
    Kad = 0x0100,
    IpfsId = 0x0200,
    IpfsPush = 0x0201,
    IpfsDelta = 0x0202,
    PeerExchange = 0x0300,
    BitswapExchange = 0x0301,
    NodeStatus = 0x0302,
    Meshsub = 0x0400,
    Rpc = 0x0500,
}

impl FromStr for StreamKind {
    // TODO: use nevertype `!`
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "/noise" => Ok(StreamKind::Handshake),
            "/coda/kad/1.0.0" => Ok(StreamKind::Kad),
            "/ipfs/id/1.0.0" => Ok(StreamKind::IpfsId),
            "/ipfs/id/push/1.0.0" => Ok(StreamKind::IpfsPush),
            "/p2p/id/delta/1.0.0" => Ok(StreamKind::IpfsDelta),
            "/mina/peer-exchange" => Ok(StreamKind::PeerExchange),
            "/mina/bitswap-exchange" => Ok(StreamKind::BitswapExchange),
            "/mina/node-status" => Ok(StreamKind::NodeStatus),
            "/meshsub/1.1.0" => Ok(StreamKind::Meshsub),
            "coda/rpcs/0.0.1" => Ok(StreamKind::Rpc),
            _ => Ok(StreamKind::Unknown),
        }
    }
}

impl StreamKind {
    pub fn iter() -> impl Iterator<Item = Self> {
        [
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

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StreamId {
    Handshake,
    Forward(u64),
    Backward(u64),
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamId::Handshake => write!(f, "handshake"),
            StreamId::Forward(a) => write!(f, "forward_{a:08x}"),
            StreamId::Backward(a) => write!(f, "backward_{a:08x}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Absorb, Emit, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MessageId(pub u64);

#[derive(Clone, Absorb, Serialize, Emit)]
pub struct Message {
    pub connection_id: ConnectionId,
    #[custom_absorb(custom_coding::stream_meta_absorb)]
    #[custom_emit(custom_coding::stream_meta_emit)]
    pub stream_id: StreamId,
    #[custom_absorb(custom_coding::stream_kind_absorb)]
    #[custom_emit(custom_coding::stream_kind_emit)]
    pub stream_kind: StreamKind,
    pub incoming: bool,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub timestamp: SystemTime,
    pub offset: u64,
    pub size: u32,
}

impl AsRef<SystemTime> for Message {
    fn as_ref(&self) -> &SystemTime {
        &self.timestamp
    }
}

#[derive(Serialize)]
pub struct FullMessage {
    pub connection_id: ConnectionId,
    pub incoming: bool,
    pub timestamp: SystemTime,
    pub stream_id: StreamId,
    pub stream_kind: StreamKind,
    // dynamic type, the type is depend on `stream_kind`
    pub message: serde_json::Value,
}
