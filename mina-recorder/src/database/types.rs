use std::{
    time::{SystemTime, Duration, UNIX_EPOCH},
    fmt,
    str::FromStr,
    net::SocketAddr,
    ops::AddAssign,
};

use radiation::{Absorb, Emit};

use serde::{Serialize};

use crate::{event::ConnectionInfo, custom_coding, strace::StraceLine};

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
            .unwrap()
            .insert("stats_in".to_owned(), stats_in);
        v.as_object_mut()
            .unwrap()
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
            .unwrap()
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
#[derive(Clone, Copy, PartialEq, Eq)]
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

#[derive(Default, Clone, Copy, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Serialize)]
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
            .unwrap()
    }
}

impl Timestamp for FullMessage {
    fn timestamp(&self) -> Duration {
        self.timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
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
            .unwrap()
    }
}

impl Timestamp for serde_json::Value {
    fn timestamp(&self) -> Duration {
        fn inner(v: &serde_json::Value) -> Option<SystemTime> {
            let timestamp = v.as_object()?.get("timestamp")?;
            serde_json::from_value(timestamp.clone()).ok()
        }

        match inner(self) {
            Some(timestamp) => timestamp.duration_since(SystemTime::UNIX_EPOCH).unwrap(),
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
