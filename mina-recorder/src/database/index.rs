use radiation::{Absorb, Emit};

use crate::decode::MessageType;
use super::types::{ConnectionId, MessageId, StreamFullId, StreamKind};

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
