use std::net::SocketAddr;

use radiation::{Absorb, Emit};

use crate::{decode::MessageType, custom_coding};
use super::types::{ConnectionId, MessageId, StreamFullId, StreamKind};

#[derive(Absorb, Emit)]
pub struct AddressIdx {
    #[custom_absorb(custom_coding::addr_absorb)]
    #[custom_emit(custom_coding::addr_emit)]
    pub addr: SocketAddr,
    pub id: MessageId,
}

#[derive(Absorb, Emit)]
pub struct ConnectionIdx {
    pub connection_id: ConnectionId,
    pub id: MessageId,
}

#[derive(Absorb, Emit)]
pub struct StreamIdx {
    pub stream_full_id: StreamFullId,
    pub id: MessageId,
}

#[derive(Absorb, Emit)]
pub struct StreamByKindIdx {
    pub stream_kind: StreamKind,
    pub id: MessageId,
}

#[derive(Absorb, Emit)]
pub struct MessageKindIdx {
    pub ty: MessageType,
    pub id: MessageId,
}

#[derive(Absorb, Emit)]
pub struct LedgerHashIdx {
    pub hash: LedgerHash,
    pub offset: u64,
    pub size: u64,
    pub id: StreamFullId,
}

impl LedgerHashIdx {
    fn _31(h: mina_p2p_messages::v2::LedgerHash) -> [u8; 31] {
        let mut hash = [0; 31];
        hash.clone_from_slice(&h.into_inner().0.as_ref()[1..]);
        hash
    }

    fn new(hash: LedgerHash) -> Self {
        LedgerHashIdx {
            hash,
            offset: 0,
            size: 0,
            id: StreamFullId {
                cn: ConnectionId(0),
                id: super::StreamId::Handshake,
            },
        }
    }

    pub fn get_31(&self) -> &[u8; 31] {
        match &self.hash {
            LedgerHash::Source(x) => x,
            LedgerHash::Target(x) => x,
            LedgerHash::FirstSource(x) => x,
            LedgerHash::FirstTargetSecondSource(x) => x,
            LedgerHash::SecondTarget(x) => x,
        }
    }

    pub fn source(h: mina_p2p_messages::v2::LedgerHash) -> Self {
        Self::new(LedgerHash::Source(Self::_31(h)))
    }

    pub fn target(h: mina_p2p_messages::v2::LedgerHash) -> Self {
        Self::new(LedgerHash::Target(Self::_31(h)))
    }

    pub fn first_source(h: mina_p2p_messages::v2::LedgerHash) -> Self {
        Self::new(LedgerHash::FirstSource(Self::_31(h)))
    }

    pub fn middle(h: mina_p2p_messages::v2::LedgerHash) -> Self {
        Self::new(LedgerHash::FirstTargetSecondSource(Self::_31(h)))
    }

    pub fn second_target(h: mina_p2p_messages::v2::LedgerHash) -> Self {
        Self::new(LedgerHash::SecondTarget(Self::_31(h)))
    }
}

#[derive(Absorb, Emit)]
#[tag(u8)]
pub enum LedgerHash {
    Source([u8; 31]),
    Target([u8; 31]),
    FirstSource([u8; 31]),
    FirstTargetSecondSource([u8; 31]),
    SecondTarget([u8; 31]),
}
