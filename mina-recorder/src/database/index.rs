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
