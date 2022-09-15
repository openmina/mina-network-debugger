use std::mem;

use crate::database::{DbStream, StreamId, StreamKind};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

pub struct State<Inner> {
    stream_id: u64,
    stream_forward: bool,
    incoming: Option<String>,
    outgoing: Option<String>,
    accumulator_incoming: Vec<u8>,
    accumulator_outgoing: Vec<u8>,
    error: bool,
    stream: Option<DbStream>,
    inner: Option<Inner>,
}

impl<Inner> From<(u64, bool)> for State<Inner> {
    fn from((stream_id, stream_forward): (u64, bool)) -> Self {
        State {
            stream_id,
            stream_forward,
            incoming: None,
            outgoing: None,
            accumulator_incoming: vec![],
            accumulator_outgoing: vec![],
            error: false,
            stream: None,
            inner: None,
        }
    }
}

fn take_msg<'a>(cursor: &mut &'a [u8]) -> Option<&'a [u8]> {
    // TODO: unsigned variable-length integer
    // https://github.com/multiformats/unsigned-varint
    let length = *(*cursor).first()? as usize;
    if cursor.len() < length + 1 {
        return None;
    }
    *cursor = &cursor[1..];

    let (msg, remaining) = cursor.split_at(length);
    *cursor = remaining;
    Some(msg)
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + DynamicProtocol,
{
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        if self.error {
            return Ok(());
        }

        let (accumulator, done, other) = if id.incoming {
            (
                &mut self.accumulator_incoming,
                &mut self.incoming,
                &self.outgoing,
            )
        } else {
            (
                &mut self.accumulator_outgoing,
                &mut self.outgoing,
                &self.incoming,
            )
        };
        let other = other.as_ref().map(String::as_str).unwrap_or("none");
        if let Some(protocol) = done {
            let inner = self.inner.get_or_insert_with(|| {
                Inner::from_name(protocol, self.stream_id, self.stream_forward)
            });
            if accumulator.is_empty() {
                inner.on_data(id, bytes, cx, db)
            } else {
                let mut total = mem::take(accumulator);
                total.extend_from_slice(bytes);
                inner.on_data(id, &mut total, cx, db)
            }
        } else {
            accumulator.extend_from_slice(bytes);
            let cursor = &mut accumulator.as_slice();
            while let Some(msg) = take_msg(cursor) {
                if let Ok(s) = std::str::from_utf8(msg) {
                    let s = s.trim_end_matches('\n');
                    if s.starts_with("/multistream/") || s == "na" {
                        continue;
                    }
                    if s.starts_with("/libp2p/simultaneous-connect") {
                        // TODO: handle
                        continue;
                    }
                    let stream = self.stream.get_or_insert_with(|| {
                        let stream_id = if self.stream_forward {
                            StreamId::Forward(self.stream_id)
                        } else {
                            StreamId::Backward(self.stream_id)
                        };
                        db.add(stream_id, StreamKind::Select)
                    });
                    stream.add(id.incoming, id.metadata.time, s.as_bytes())?;
                    *done = Some(s.to_string());
                    break;
                } else {
                    log::error!(
                        "{id}, {}, stream_id: {}, other: {other}, unparsed {}",
                        db.id(),
                        self.stream_id,
                        hex::encode(msg)
                    );
                    self.error = true;
                    return Ok(());
                }
            }
            *accumulator = (*cursor).to_vec();
            Ok(())
        }
    }
}
