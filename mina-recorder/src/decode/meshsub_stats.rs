use std::{
    time::{SystemTime, Duration},
    fmt,
    str::FromStr,
};

use libp2p_core::PeerId;
use mina_p2p_messages::{bigint::BigInt, v2};
use radiation::{Absorb, Emit};
use serde::{Serialize, Deserialize};

use crate::custom_coding;

use super::MessageType;

#[derive(Default, Clone, Absorb, Emit, Serialize, Deserialize)]
pub struct BlockStat {
    pub height: u32,
    pub events: Vec<Event>,
}

impl BlockStat {
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

#[derive(Clone, Absorb, Emit, Serialize, Deserialize)]
pub struct Event {
    #[custom_absorb(custom_coding::peer_id_absorb)]
    #[custom_emit(custom_coding::peer_id_emit)]
    pub producer_id: PeerId,
    pub hash: Hash,
    pub block_height: u32,
    pub global_slot: u32,
    // TODO: group
    pub incoming: bool,
    pub message_kind: MessageType,
    pub message_id: u64,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub time: SystemTime,
    #[custom_absorb(custom_coding::duration_opt_absorb)]
    #[custom_emit(custom_coding::duration_opt_emit)]
    pub latency: Option<Duration>,
    pub sender_addr: String,
    pub receiver_addr: String,
}

#[derive(Clone, Absorb, Emit, Serialize)]
pub struct TxStat {
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub block_time: SystemTime,
    pub block_height: u32,

    pub transactions: Vec<Tx>,
    pub snarks: Vec<Snark>,

    pub pending_txs: Vec<u64>,
    // pub pending_snarks: Vec<u64>,
}

#[derive(Clone, Absorb, Emit, Serialize)]
pub struct Tx {
    #[custom_absorb(custom_coding::peer_id_absorb)]
    #[custom_emit(custom_coding::peer_id_emit)]
    pub producer_id: PeerId,
    // pub hash: Hash,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub time: SystemTime,

    #[custom_absorb(custom_coding::binprot_absorb)]
    #[custom_emit(custom_coding::binprot_emit)]
    pub command: v2::StagedLedgerDiffDiffPreDiffWithAtMostTwoCoinbaseStableV2B,

    #[custom_absorb(custom_coding::duration_absorb)]
    #[custom_emit(custom_coding::duration_emit)]
    pub latency: Duration,
}

#[derive(Clone, Absorb, Emit, Serialize)]
pub struct Snark {
    #[custom_absorb(custom_coding::peer_id_absorb)]
    #[custom_emit(custom_coding::peer_id_emit)]
    pub producer_id: PeerId,
    // pub hash: Hash,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub time: SystemTime,

    #[custom_absorb(custom_coding::binprot_absorb)]
    #[custom_emit(custom_coding::binprot_emit)]
    pub work: v2::TransactionSnarkWorkTStableV2,

    #[custom_absorb(custom_coding::duration_absorb)]
    #[custom_emit(custom_coding::duration_emit)]
    pub latency: Duration,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Absorb, Emit)]
pub struct Hash(pub [u8; 32]);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Absorb, Emit)]
pub struct Signature(pub [u8; 32], pub [u8; 32]);

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        hex::encode(&self.0).serialize(serializer)
    }
}

impl FromStr for Hash {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let v = hex::decode(s)?;
        if v.len() != 32 {
            Err(hex::FromHexError::InvalidStringLength)
        } else {
            let mut s = Hash([0; 32]);
            s.0.clone_from_slice(&v);
            Ok(s)
        }
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).and_then(|s| s.parse().map_err(serde::de::Error::custom))
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
