use radiation::{Absorb, Emit};

use super::types::{ConnectionId, MessageId};

#[derive(Clone, Debug, Absorb, Emit)]
pub struct Connection {
    pub connection_id: ConnectionId,
    pub id: MessageId,
}
