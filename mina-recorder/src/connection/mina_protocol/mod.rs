use std::collections::BTreeMap;
use std::io::Cursor;
use binprot::{BinProtRead, BinProtWrite};
use mina_p2p_messages::{
    string::CharString as BString,
    rpc_kernel::{QueryHeader, MessageHeader},
    utils,
};

use crate::database::{StreamId, StreamKind, DbStream};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

pub struct State {
    stream_id: StreamId,
    kind: StreamKind,
    rpc_context: BTreeMap<i64, (BString, i32)>,
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
            rpc_context: BTreeMap::default(),
            stream: None,
        }
    }
}

impl HandleData for State {
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _cx: &mut Cx, db: &Db) -> DbResult<()> {
        let stream = self
            .stream
            .get_or_insert_with(|| db.add(self.stream_id, self.kind));
        if self.kind == StreamKind::Rpc {
            let mut s = Cursor::new(bytes);
            let len = match utils::stream_decode_size(&mut s) {
                Ok(v) => v,
                Err(err) => {
                    log::error!(
                        "rpc message slice too short {err}, {}",
                        hex::encode(s.get_ref())
                    );
                    return Ok(());
                }
            };
            match MessageHeader::binprot_read(&mut s) {
                Err(err) => {
                    log::error!("{err}");
                }
                Ok(MessageHeader::Heartbeat) => (),
                Ok(MessageHeader::Query(v)) => {
                    self.rpc_context.insert(v.id, (v.tag, v.version));
                    stream.add(id.incoming, id.metadata.time, &s.get_ref()[..(8 + len)])?;
                }
                Ok(MessageHeader::Response(v)) => {
                    let pos = s.position();
                    if let Some((tag, version)) = self.rpc_context.remove(&v.id) {
                        let q = QueryHeader {
                            tag,
                            version,
                            id: v.id,
                        };
                        let mut b = [0; 8].to_vec();
                        b.push(2);
                        q.binprot_write(&mut b).unwrap();
                        let new_len = (len + b.len()) as u64 - pos;
                        b[0..8].clone_from_slice(&new_len.to_le_bytes());
                        b.extend_from_slice(&s.get_ref()[(pos as usize)..(8 + len)]);
                        stream.add(id.incoming, id.metadata.time, &b)?;
                    } else {
                        // magic number, means kind of rpc handshake
                        if v.id != 4411474 {
                            log::warn!("{id}, response {} without request", v.id);
                        }
                    }
                }
            }

            let rest = &mut s.get_mut()[(8 + len)..];
            if !rest.is_empty() {
                self.on_data(id, rest, _cx, db)
            } else {
                Ok(())
            }
        } else {
            stream.add(id.incoming, id.metadata.time, bytes)
        }
    }
}
