pub mod meshsub;
pub mod kademlia;

use crate::database::{StreamMeta, StreamKind, DbStream};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db};

pub struct State {
    meta: StreamMeta,
    kind: StreamKind,
    stream: Option<DbStream>,
}

impl DynamicProtocol for State {
    fn from_name(name: &str, id: u64, forward: bool) -> Self {
        let meta = if forward {
            StreamMeta::Forward(id)
        } else {
            StreamMeta::Backward(id)
        };
        State {
            meta,
            kind: name.parse().unwrap(),
            stream: None,
        }
    }
}

impl HandleData for State {
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _: &mut Cx, db: &Db) {
        let stream = self
            .stream
            .get_or_insert_with(|| db.add(self.meta, self.kind).unwrap());
        stream.add(id.incoming, id.metadata.time, bytes).unwrap();
    }
}
