use std::fmt;

use binprot::{BinProtRead, BinProtWrite};
use mina_p2p_messages::gossip::GossipNetMessageV2;

pub enum ConsensusMessage {
    Inner {
        inner: GossipNetMessageV2,
        hash: String,
    },
    Test(String),
}

impl ConsensusMessage {
    pub fn from_bytes(bytes: &[u8], topic: &str) -> Result<Self, binprot::Error> {
        let mut bytes_cut = &bytes[8..];
        if bytes_cut[0] == 3 {
            let msg = String::from_utf8(bytes_cut[1..].to_vec()).unwrap_or(String::new());
            Ok(ConsensusMessage::Test(msg))
        } else {
            GossipNetMessageV2::binprot_read(&mut bytes_cut).map(|inner| Self::Inner {
                inner,
                hash: hex::encode(Self::calc_hash(bytes, topic)),
            })
        }
    }

    pub fn calc_hash(data: &[u8], topic: &str) -> [u8; 32] {
        use blake2::digest::{typenum, FixedOutput, Mac, Update};

        let key;
        let key = if topic.as_bytes().len() <= 64 {
            topic.as_bytes()
        } else {
            key = blake2::Blake2b::<typenum::U32>::default()
                .chain(topic.as_bytes())
                .finalize_fixed();
            key.as_slice()
        };
        blake2::Blake2bMac::<typenum::U32>::new_from_slice(key)
            .unwrap()
            .chain(data)
            .finalize_fixed()
            .into()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Self::Test(msg) => {
                let mut v = vec![0, 0, 0, 0, 0, 0, 0, 0, 3];
                v.extend_from_slice(msg.as_bytes());
                let l = ((v.len() - 8) as u64).to_be_bytes();
                v[..8].clone_from_slice(&l);
                v
            }
            Self::Inner { inner, .. } => {
                let mut v = vec![0; 8];
                inner.binprot_write(&mut v).expect("encode");
                let l = ((v.len() - 8) as u64).to_be_bytes();
                v[..8].clone_from_slice(&l);
                v
            }
        }
    }

    pub fn relevant(&self) -> bool {
        matches!(
            self,
            Self::Inner {
                inner: GossipNetMessageV2::NewState(_),
                ..
            } | Self::Test(_)
        )
    }
}

impl fmt::Debug for ConsensusMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for ConsensusMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inner {
                inner: GossipNetMessageV2::NewState(state),
                ..
            } => {
                let height = state
                    .header
                    .protocol_state
                    .body
                    .consensus_state
                    .blockchain_length
                    .0
                     .0 as u32;

                write!(f, "new state: {height}")
            }
            Self::Inner {
                inner: GossipNetMessageV2::SnarkPoolDiff(_),
                ..
            } => write!(f, "snark pool diff"),
            Self::Inner {
                inner: GossipNetMessageV2::TransactionPoolDiff(_),
                ..
            } => {
                write!(f, "transaction pool diff")
            }
            Self::Test(msg) => write!(f, "test: {msg}"),
        }
    }
}
