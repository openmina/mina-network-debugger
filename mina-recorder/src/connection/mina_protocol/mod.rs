use crate::database::{StreamId, StreamKind, DbStream};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

pub struct State {
    stream_id: StreamId,
    kind: StreamKind,
    stream: Option<DbStream>,
}

impl DynamicProtocol for State {
    fn from_name(name: &str, id: u64, forward: bool) -> Self {
        let stream_id = if forward {
            StreamId::Forward(id)
        } else {
            StreamId::Backward(id)
        };
        State {
            stream_id,
            kind: name.parse().expect("cannot fail"),
            stream: None,
        }
    }
}

impl HandleData for State {
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _: &mut Cx, db: &Db) -> DbResult<()> {
        let stream = self
            .stream
            .get_or_insert_with(|| db.add(self.stream_id, self.kind));
        // WARNING: skip empty rpc
        if let StreamKind::Rpc = self.kind {
            if bytes[0] == 1 {
                return Ok(());
            }
        }
        stream.add(id.incoming, id.metadata.time, bytes)
    }
}
