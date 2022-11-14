use std::{time::SystemTime, fmt, net::SocketAddr};

use libp2p_core::PeerId;
use mina_p2p_messages::bigint::BigInt;
use radiation::{Absorb, Emit, AbsorbExt};
use serde::Serialize;

use super::DecodeError;
use crate::custom_coding;

#[derive(Default, Clone, Absorb, Emit, Serialize)]
pub struct T {
    pub height: u32,
    pub events: Vec<Event>,
}

#[derive(Clone, Absorb, Emit, Serialize)]
pub struct Event {
    pub kind: Kind,
    #[serde(serialize_with = "custom_coding::serialize_peer_id")]
    #[custom_absorb(custom_coding::peer_id_absorb)]
    #[custom_emit(custom_coding::peer_id_emit)]
    pub producer_id: PeerId,
    pub hash: Hash,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub time: SystemTime,
    #[custom_absorb(custom_coding::addr_absorb)]
    #[custom_emit(custom_coding::addr_emit)]
    pub peer: SocketAddr,
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

pub fn parse(bytes: Vec<u8>, _: bool) -> Result<serde_json::Value, DecodeError> {
    let t = T::absorb_ext(&bytes)?;

    serde_json::to_value(&t).map_err(DecodeError::Serde)
}
