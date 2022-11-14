use std::{time::{SystemTime, Duration}, fmt, net::SocketAddr};

use libp2p_core::PeerId;
use mina_p2p_messages::bigint::BigInt;
use radiation::{Absorb, Emit};
use serde::Serialize;

use crate::custom_coding;

#[derive(Default, Clone, Absorb, Emit, Serialize)]
pub struct T {
    pub height: u32,
    pub receive_latency: Vec<Latency>,
    pub send_latency: Vec<Latency>,
    pub events: Vec<Event>,
}

impl T {
    pub fn clear(&mut self) {
        self.receive_latency.clear();
        self.send_latency.clear();
        self.events.clear();
    }
}

#[derive(Clone, Absorb, Emit, Serialize)]
pub struct Latency {
    #[serde(serialize_with = "custom_coding::serialize_peer_id")]
    #[custom_absorb(custom_coding::peer_id_absorb)]
    #[custom_emit(custom_coding::peer_id_emit)]
    pub producer_id: PeerId,
    pub hash: Hash,
    #[custom_absorb(custom_coding::addr_absorb)]
    #[custom_emit(custom_coding::addr_emit)]
    pub peer_addr: SocketAddr,
    #[custom_absorb(custom_coding::duration_absorb)]
    #[custom_emit(custom_coding::duration_emit)]
    pub latency: Duration,
    // pub is_control_message: bool,
}

#[derive(Clone, Absorb, Emit, Serialize)]
pub struct Event {
    pub kind: Kind,
    #[serde(serialize_with = "custom_coding::serialize_peer_id")]
    #[custom_absorb(custom_coding::peer_id_absorb)]
    #[custom_emit(custom_coding::peer_id_emit)]
    pub producer_id: PeerId,
    pub hash: Hash,
    pub message_id: u64,
    pub block_height: u32,
    pub global_slot: u32,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub time: SystemTime,
    pub sender_addr: String,
    pub receiver_addr: String,
}

#[derive(Clone, Absorb, Emit, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    SendIHave,
    SendIWant,
    SendValue,
    RecvIHave,
    RecvIWant,
    RecvValue,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Absorb, Emit)]
pub struct Hash(pub [u8; 32]);

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        hex::encode(&self.0).serialize(serializer)
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.0))
    }
}

impl From<BigInt> for Hash {
    fn from(v: BigInt) -> Self {
        Hash(*Box::from(v))
    }
}
