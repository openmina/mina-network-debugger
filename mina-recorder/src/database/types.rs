use std::{
    time::{SystemTime, Duration, UNIX_EPOCH},
    fmt,
    str::FromStr,
    net::SocketAddr,
    ops::AddAssign,
};

use mina_p2p_messages::{binprot::BinProtRead, v2, gossip::GossipNetMessageV2};
use radiation::{Absorb, Emit};

use serde::{Serialize, Deserialize};

use crate::{
    event::ConnectionInfo, custom_coding, strace::StraceLine, libp2p_helper::CapnpEvent,
    meshsub_stats::Hash,
};

#[derive(
    Clone, Copy, Debug, Absorb, Emit, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
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

    #[serde(skip)]
    pub stats_in: ConnectionStats,
    #[serde(skip)]
    pub stats_out: ConnectionStats,

    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub timestamp_close: SystemTime,

    pub alias: String,
}

impl Connection {
    pub fn post_process(&self, now: Option<SystemTime>) -> serde_json::Value {
        let end = if self.timestamp_close == UNIX_EPOCH {
            now.unwrap_or_else(SystemTime::now)
        } else {
            self.timestamp_close
        };
        let duration = end.duration_since(self.timestamp).expect("must not fail");
        let stats_in = self.stats_in.calc_speed(duration);
        let stats_out = self.stats_out.calc_speed(duration);
        let mut v = serde_json::to_value(self).expect("must not fail");
        v.as_object_mut()
            .expect("self must be a structure")
            .insert("stats_in".to_owned(), stats_in);
        v.as_object_mut()
            .expect("self must be a structure")
            .insert("stats_out".to_owned(), stats_out);

        v
    }
}

#[derive(Default, Clone, Absorb, Emit, Serialize)]
pub struct ConnectionStats {
    pub total_bytes: u64,
    pub decrypted_bytes: u64,
    pub decrypted_chunks: u64,
    pub messages: u64,
}

impl ConnectionStats {
    pub fn calc_speed(&self, duration: Duration) -> serde_json::Value {
        let speed = self.total_bytes as f64 / duration.as_secs_f64();
        let speed = serde_json::to_value(speed).expect("must not fail");
        let mut v = serde_json::to_value(self).expect("must not fail");
        v.as_object_mut()
            .expect("self must be a structure")
            .insert("speed".to_string(), speed);
        v
    }
}

impl AddAssign<ConnectionStats> for ConnectionStats {
    fn add_assign(&mut self, rhs: ConnectionStats) {
        self.total_bytes += rhs.total_bytes;
        self.decrypted_bytes += rhs.decrypted_bytes;
        self.decrypted_chunks += rhs.decrypted_chunks;
        self.messages += rhs.messages;
    }
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    Select = 0x0600,
    Mplex = 0x0700,
    Yamux = 0x0701,
}

impl fmt::Display for StreamKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamKind::Handshake => write!(f, "/noise"),
            StreamKind::Kad => write!(f, "/coda/kad/1.0.0"),
            StreamKind::IpfsId => write!(f, "/ipfs/id/1.0.0"),
            StreamKind::IpfsPush => write!(f, "/ipfs/id/push/1.0.0"),
            StreamKind::IpfsDelta => write!(f, "/p2p/id/delta/1.0.0"),
            StreamKind::PeerExchange => write!(f, "/mina/peer-exchange"),
            StreamKind::BitswapExchange => write!(f, "/mina/bitswap-exchange"),
            StreamKind::NodeStatus => write!(f, "/mina/node-status"),
            StreamKind::Meshsub => write!(f, "/meshsub/1.1.0"),
            StreamKind::Rpc => write!(f, "coda/rpcs/0.0.1"),
            StreamKind::Select => write!(f, "/multistream/1.0.0"),
            StreamKind::Mplex => write!(f, "/coda/mplex/1.0.0"),
            StreamKind::Yamux => write!(f, "/coda/yamux/1.0.0"),
            StreamKind::Unknown => write!(f, "unknown"),
        }
    }
}

impl Serialize for StreamKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StreamKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|s| s.parse().expect("cannot fail"))
    }
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
            "/multistream/1.0.0" => Ok(StreamKind::Select),
            "/coda/mplex/1.0.0" => Ok(StreamKind::Mplex),
            "/coda/yamux/1.0.0" => Ok(StreamKind::Yamux),
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
            StreamKind::IpfsDelta,
            StreamKind::PeerExchange,
            StreamKind::BitswapExchange,
            StreamKind::NodeStatus,
            StreamKind::Meshsub,
            StreamKind::Rpc,
            StreamKind::Select,
            StreamKind::Mplex,
            StreamKind::Yamux,
            StreamKind::Unknown,
        ]
        .into_iter()
    }
}

#[derive(Default, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StreamId {
    #[default]
    Handshake,
    Forward(u64),
    Backward(u64),
}

impl FromStr for StreamId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "handshake" {
            Ok(StreamId::Handshake)
        } else if s.starts_with("forward_") {
            let i = u64::from_str_radix(s.trim_start_matches("forward_"), 16)
                .map_err(|err| err.to_string())?;
            Ok(StreamId::Forward(i))
        } else if s.starts_with("backward_") {
            let i = u64::from_str_radix(s.trim_start_matches("backward_"), 16)
                .map_err(|err| err.to_string())?;
            Ok(StreamId::Backward(i))
        } else {
            Err(s.to_string())
        }
    }
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
    pub stream_id: StreamId,
    pub stream_kind: StreamKind,
    pub incoming: bool,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub timestamp: SystemTime,
    pub offset: u64,
    pub size: u32,
    pub brief: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FullMessage {
    pub connection_id: ConnectionId,
    pub remote_addr: SocketAddr,
    pub incoming: bool,
    pub timestamp: SystemTime,
    pub stream_id: StreamId,
    pub stream_kind: StreamKind,
    // dynamic type, the type is depend on `stream_kind`
    pub message: serde_json::Value,
    pub size: u32,
}

pub trait Timestamp {
    fn timestamp(&self) -> Duration;
}

impl Timestamp for Message {
    fn timestamp(&self) -> Duration {
        self.timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp cannot be earlier the `UNIX_EPOCH`")
    }
}

impl Timestamp for FullMessage {
    fn timestamp(&self) -> Duration {
        self.timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp cannot be earlier the `UNIX_EPOCH`")
    }
}

impl Timestamp for StraceLine {
    fn timestamp(&self) -> Duration {
        self.start
    }
}

impl Timestamp for Connection {
    fn timestamp(&self) -> Duration {
        self.timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("timestamp cannot be earlier the `UNIX_EPOCH`")
    }
}

impl Timestamp for serde_json::Value {
    fn timestamp(&self) -> Duration {
        fn inner(v: &serde_json::Value) -> Option<SystemTime> {
            let timestamp = v.as_object()?.get("timestamp")?;
            serde_json::from_value(timestamp.clone()).ok()
        }

        match inner(self) {
            Some(timestamp) => timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("timestamp cannot be earlier the `UNIX_EPOCH`"),
            None => Duration::ZERO,
        }
    }
}

mod implementations {
    use radiation::{Absorb, Emit, nom, ParseError, Limit};

    use super::{StreamId, StreamKind};

    impl<'pa> Absorb<'pa> for StreamKind {
        fn absorb<L>(input: &'pa [u8]) -> nom::IResult<&'pa [u8], Self, ParseError<&'pa [u8]>>
        where
            L: Limit,
        {
            nom::combinator::map(u16::absorb::<()>, |d| {
                for v in StreamKind::iter() {
                    if d == v as u16 {
                        return v;
                    }
                }
                StreamKind::Unknown
            })(input)
        }
    }

    impl<W> Emit<W> for StreamKind
    where
        W: for<'a> Extend<&'a u8>,
    {
        fn emit(&self, buffer: &mut W) {
            (*self as u16).emit(buffer);
        }
    }

    impl<'pa> Absorb<'pa> for StreamId {
        fn absorb<L>(input: &'pa [u8]) -> nom::IResult<&'pa [u8], Self, ParseError<&'pa [u8]>>
        where
            L: Limit,
        {
            nom::combinator::map(i64::absorb::<()>, |d| match d {
                i64::MIN => StreamId::Handshake,
                d => {
                    if d >= 0 {
                        StreamId::Forward(d as u64)
                    } else {
                        StreamId::Backward((-d - 1) as u64)
                    }
                }
            })(input)
        }
    }

    impl<W> Emit<W> for StreamId
    where
        W: for<'a> Extend<&'a u8>,
    {
        fn emit(&self, buffer: &mut W) {
            let d = match self {
                StreamId::Handshake => i64::MIN,
                StreamId::Forward(s) => *s as i64,
                StreamId::Backward(s) => -((*s + 1) as i64),
            };
            d.emit(buffer);
        }
    }
}

#[derive(Absorb)]
pub struct Time(#[custom_absorb(custom_coding::time_absorb)] SystemTime);

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use time::OffsetDateTime;

        let (hour, minute, second, nano) = OffsetDateTime::from(self.0).time().as_hms_nano();

        write!(f, "{hour:02}:{minute:02}:{second:02}.{nano:09}")
    }
}

impl Serialize for Time {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

#[derive(Emit, Absorb)]
pub struct StatsDbKey {
    pub height: u32,
    #[custom_emit(custom_coding::addr_emit)]
    #[custom_absorb(custom_coding::addr_absorb)]
    pub node_address: SocketAddr,
}

#[derive(Emit, Absorb)]
pub struct StatsV2DbKey {
    pub height: u32,
    #[custom_emit(custom_coding::time_emit)]
    #[custom_absorb(custom_coding::time_absorb)]
    pub time: SystemTime,
}

impl fmt::Display for StatsDbKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.height, self.node_address)
    }
}

impl fmt::Display for StatsV2DbKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}",
            self.height,
            self.time
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("timestamp cannot be earlier the `UNIX_EPOCH`")
                .as_nanos()
        )
    }
}

#[derive(Emit, Absorb)]
pub struct CapnpEventWithMetadataKey {
    pub height: u32,
    #[custom_emit(custom_coding::time_emit)]
    #[custom_absorb(custom_coding::time_absorb)]
    pub time: SystemTime,
}

impl fmt::Display for CapnpEventWithMetadataKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.time)
    }
}

#[derive(Emit, Absorb)]
pub struct CapnpEventWithMetadata {
    #[custom_emit(custom_coding::time_emit)]
    #[custom_absorb(custom_coding::time_absorb)]
    pub real_time: SystemTime,
    #[custom_emit(custom_coding::addr_emit)]
    #[custom_absorb(custom_coding::addr_absorb)]
    pub node_address: SocketAddr,
    pub events: Vec<CapnpEvent>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum GossipNetMessageV2Short {
    NewState {
        height: u32,
    },
    TestMessage {
        height: u32,
    },
    SnarkPoolDiff,
    TransactionPoolDiff {
        inner: v2::NetworkPoolTransactionPoolDiffVersionedStableV2,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum CapnpEventDecoded {
    ReceivedGossip {
        peer_id: String,
        peer_address: String,
        msg: GossipNetMessageV2Short,
        hash: Hash,
    },
    PublishGossip {
        msg: GossipNetMessageV2Short,
        hash: Hash,
    },
}

#[derive(Serialize)]
pub struct CapnpTableRow {
    pub time_microseconds: u64,
    pub real_time_microseconds: u64,
    pub node_address: SocketAddr,
    pub events: Vec<CapnpEventDecoded>,
}

impl CapnpTableRow {
    pub fn transform(k: CapnpEventWithMetadataKey, v: CapnpEventWithMetadata) -> Self {
        CapnpTableRow {
            time_microseconds: k
                .time
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("msg")
                .as_micros() as u64,
            real_time_microseconds: v
                .real_time
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("msg")
                .as_micros() as u64,
            node_address: v.node_address,
            events: v
                .events
                .into_iter()
                .filter_map(|event| match event {
                    CapnpEvent::Publish { msg, hash } => {
                        let mut slice = msg.as_slice();
                        match GossipNetMessageV2::binprot_read(&mut slice) {
                            Ok(GossipNetMessageV2::NewState(block)) => {
                                let height = block
                                    .header
                                    .protocol_state
                                    .body
                                    .consensus_state
                                    .blockchain_length
                                    .0
                                     .0 as u32;
                                let msg = GossipNetMessageV2Short::NewState { height };
                                let hash = Hash(hash);
                                Some(CapnpEventDecoded::PublishGossip { msg, hash })
                            }
                            Ok(GossipNetMessageV2::SnarkPoolDiff { .. }) => {
                                let msg = GossipNetMessageV2Short::SnarkPoolDiff;
                                let hash = Hash(hash);
                                Some(CapnpEventDecoded::PublishGossip { msg, hash })
                            }
                            Ok(GossipNetMessageV2::TransactionPoolDiff { message, .. }) => {
                                let msg =
                                    GossipNetMessageV2Short::TransactionPoolDiff { inner: message };
                                let hash = Hash(hash);
                                Some(CapnpEventDecoded::PublishGossip { msg, hash })
                            }
                            Err(err) => {
                                if let Some(3) = msg.as_slice().first() {
                                    let msg =
                                        GossipNetMessageV2Short::TestMessage { height: k.height };
                                    let hash = Hash(hash);
                                    Some(CapnpEventDecoded::PublishGossip { msg, hash })
                                } else {
                                    log::error!("capnp decode {err}");
                                    None
                                }
                            }
                        }
                    }
                    CapnpEvent::ReceivedGossip {
                        peer_id,
                        peer_host,
                        peer_port,
                        msg,
                        hash,
                    } => {
                        let mut slice = msg.as_slice();
                        match GossipNetMessageV2::binprot_read(&mut slice) {
                            Ok(GossipNetMessageV2::NewState(block)) => {
                                let height = block
                                    .header
                                    .protocol_state
                                    .body
                                    .consensus_state
                                    .blockchain_length
                                    .0
                                     .0 as u32;
                                let msg = GossipNetMessageV2Short::NewState { height };
                                Some(CapnpEventDecoded::ReceivedGossip {
                                    peer_id,
                                    peer_address: format!("{peer_host}:{peer_port}"),
                                    msg,
                                    hash: Hash(hash),
                                })
                            }
                            Ok(GossipNetMessageV2::SnarkPoolDiff { .. }) => {
                                let msg = GossipNetMessageV2Short::SnarkPoolDiff;
                                Some(CapnpEventDecoded::ReceivedGossip {
                                    peer_id,
                                    peer_address: format!("{peer_host}:{peer_port}"),
                                    msg,
                                    hash: Hash(hash),
                                })
                            }
                            Ok(GossipNetMessageV2::TransactionPoolDiff { message, .. }) => {
                                let msg =
                                    GossipNetMessageV2Short::TransactionPoolDiff { inner: message };
                                Some(CapnpEventDecoded::ReceivedGossip {
                                    peer_id,
                                    peer_address: format!("{peer_host}:{peer_port}"),
                                    msg,
                                    hash: Hash(hash),
                                })
                            }
                            Err(err) => {
                                if let Some(3) = msg.as_slice().first() {
                                    let msg =
                                        GossipNetMessageV2Short::TestMessage { height: k.height };
                                    let hash = Hash(hash);
                                    Some(CapnpEventDecoded::ReceivedGossip {
                                        peer_id,
                                        peer_address: format!("{peer_host}:{peer_port}"),
                                        msg,
                                        hash,
                                    })
                                } else {
                                    log::error!("capnp decode {err}");
                                    None
                                }
                            }
                        }
                    }
                })
                .collect(),
        }
    }
}
