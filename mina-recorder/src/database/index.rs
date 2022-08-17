use radiation::{Absorb, Emit};

use super::types::{ConnectionId, MessageId, StreamFullId, StreamKind};

#[derive(Absorb, Emit)]
pub struct Connection {
    pub connection_id: ConnectionId,
    pub id: MessageId,
}

#[derive(Absorb, Emit)]
pub struct Stream {
    pub stream_full_id: StreamFullId,
    pub id: MessageId,
}

#[derive(Absorb, Emit)]
pub struct StreamByKind {
    pub stream_kind: StreamKind,
    pub id: MessageId,
}
