pub mod meshsub;
pub mod kademlia;

use crate::database::{StreamId, StreamKind, DbStream};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db};

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
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _: &mut Cx, db: &Db) {
        let stream = self
            .stream
            .get_or_insert_with(|| db.add(self.stream_id, self.kind));
        stream.add(id.incoming, id.metadata.time, bytes).unwrap();
    }
}
