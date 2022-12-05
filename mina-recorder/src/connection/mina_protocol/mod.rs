use super::accumulator;

mod meshsub;
mod rpc;

use crate::database::{StreamId, StreamKind, ConnectionStats, DbStream};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

pub struct State {
    stream_id: StreamId,
    kind: StreamKind,
    rpc_state: Option<rpc::State>,
    meshsub_state: Option<meshsub::State>,
}

impl DynamicProtocol for State {
    fn from_name(name: &str, stream_id: StreamId) -> Self {
        let kind = name.parse().expect("cannot fail");
        State {
            stream_id,
            kind,
            rpc_state: {
                if let StreamKind::Rpc = kind {
                    Some(rpc::State::default())
                } else {
                    None
                }
            },
            meshsub_state: {
                if let StreamKind::Meshsub = kind {
                    Some(meshsub::State::default())
                } else {
                    None
                }
            },
        }
    }
}

impl HandleData for State {
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        let stream = db.get(self.stream_id);
        if self.kind == StreamKind::Rpc {
            let st = self.rpc_state.as_mut().expect("must exist");
            match st.extend(bytes) {
                Err(err) => log::error!("{id} {}: {err}", db.id()),
                Ok(None) => loop {
                    match st.next_msg() {
                        Err(err) => log::error!("{id} {}: {err}", db.id()),
                        Ok(None) => break,
                        Ok(Some(msg)) => {
                            if let Err(err) = stream.add(&id, self.kind, &msg) {
                                log::error!("{id} {}: {err}", db.id());
                            }
                        }
                    }
                },
                Ok(Some(msg)) => {
                    if let Err(err) = stream.add(&id, self.kind, &msg) {
                        log::error!("{id} {}: {err}, {}", db.id(), hex::encode(bytes));
                    }
                }
            }
        } else if self.kind == StreamKind::Meshsub {
            let st = self.meshsub_state.as_mut().expect("must exist");
            if !st.extend(bytes) {
                meshsub_sink(&id, db, &stream, bytes, cx);
            } else {
                while let Some(slice) = st.next_msg() {
                    meshsub_sink(&id, db, &stream, slice, cx);
                }
            }
        } else {
            stream.add(&id, self.kind, bytes)?;
        }

        db.update(
            ConnectionStats {
                total_bytes: 0,
                decrypted_bytes: 0,
                decrypted_chunks: 0,
                messages: 1,
            },
            id.incoming,
        )
    }
}

fn meshsub_sink(id: &DirectedId, db: &Db, stream: &DbStream, msg: &[u8], cx: &mut Cx) {
    let port = cx
        .apps
        .get(&id.metadata.id.pid)
        .map(|(_, p)| *p)
        .unwrap_or(8302);
    if let Some(st) = cx.stats_state.get_mut(&port) {
        st.observe(
            msg,
            id.incoming,
            id.metadata.time,
            &cx.db,
            id.metadata.id.addr,
            &cx.aggregator,
            port,
        );
    }
    if let Err(err) = stream.add(id, StreamKind::Meshsub, msg) {
        log::error!("{id} {}: {err}, {}", db.id(), hex::encode(msg));
    }
}
