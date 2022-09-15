use std::{collections::BTreeMap, ops::Range};

use unsigned_varint::decode;

use crate::database::{DbStream, StreamId, StreamKind};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

pub struct State<Inner> {
    accumulator_incoming: Vec<u8>,
    accumulator_outgoing: Vec<u8>,
    stream: Option<DbStream>,
    inners: BTreeMap<MplexStreamId, Status<Inner>>,
}

pub enum Status<Inner> {
    Duplex(Inner),
    IncomingOnly(Inner),
    OutgoingOnly(Inner),
}

impl<Inner> DynamicProtocol for State<Inner> {
    fn from_name(name: &str, _: u64, _: bool) -> Self {
        assert_eq!(name, "/coda/mplex/1.0.0");
        State {
            accumulator_incoming: Vec::default(),
            accumulator_outgoing: Vec::default(),
            stream: None,
            inners: BTreeMap::default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MplexStreamId {
    pub i: u64,
    pub initiator_is_incoming: bool,
}

enum Tag {
    New,
    Msg,
    Close,
    Reset,
}

struct Header {
    tag: Tag,
    stream_id: MplexStreamId,
}

impl Header {
    fn new(v: u64, incoming: bool) -> Self {
        let initiator = v % 2 == 0;
        Header {
            tag: match v & 7 {
                0 => Tag::New,
                1 | 2 => Tag::Msg,
                3 | 4 => Tag::Close,
                5 | 6 => Tag::Reset,
                7 => panic!("wrong header tag"),
                _ => unreachable!(),
            },
            stream_id: MplexStreamId {
                i: v >> 3,
                initiator_is_incoming: initiator == incoming,
            },
        }
    }
}

impl<Inner> State<Inner>
where
    Inner: HandleData + From<(u64, bool)>,
{
    fn out(
        &mut self,
        id: DirectedId,
        cx: &mut Cx,
        db: &Db,
        header: Header,
        range: Range<usize>,
    ) -> DbResult<()> {
        let accumulator = if id.incoming {
            &mut self.accumulator_incoming
        } else {
            &mut self.accumulator_outgoing
        };
        let end = range.end;
        let bytes = &mut accumulator[range];
        let Header { tag, stream_id } = header;
        let db_stream = self.stream.get_or_insert_with(|| {
            db.add(StreamId::Select, StreamKind::Select)
        });
        match tag {
            Tag::New => {
                let name = String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| hex::encode(bytes));
                let stream = Inner::from((stream_id.i, stream_id.initiator_is_incoming));
                if self
                    .inners
                    .insert(stream_id, Status::Duplex(stream))
                    .is_some()
                {
                    log::warn!("{id}, new stream {name}, but already exist");
                }
                let b = (stream_id.i << 3).to_be_bytes();
                db_stream.add(id.incoming, id.metadata.time, &b)?;
            }
            Tag::Msg => match (self.inners.get_mut(&stream_id), id.incoming) {
                (Some(Status::Duplex(stream)), _)
                | (Some(Status::IncomingOnly(stream)), true)
                | (Some(Status::OutgoingOnly(stream)), false) => {
                    stream.on_data(id, bytes, cx, db)?
                }
                _ => log::error!(
                    "{id}, message for stream {} {} that doesn't exist",
                    stream_id.i,
                    stream_id.initiator_is_incoming,
                ),
            },
            Tag::Close => {
                match (self.inners.remove(&stream_id), id.incoming) {
                    (Some(Status::Duplex(stream)), true) => {
                        self.inners.insert(stream_id, Status::OutgoingOnly(stream));
                    }
                    (Some(Status::Duplex(stream)), false) => {
                        self.inners.insert(stream_id, Status::IncomingOnly(stream));
                    }
                    (Some(Status::IncomingOnly(_)), true) => (),
                    (Some(Status::OutgoingOnly(_)), false) => (),
                    (Some(Status::OutgoingOnly(_)), true) => {
                        log::error!("{id}, closing incoming part of stream where only outgoing part remains");
                    }
                    (Some(Status::IncomingOnly(_)), false) => {
                        log::error!("{id}, closing outgoing part of stream where only incoming part remains");
                    }
                    (None, true) => {
                        log::error!("{id}, closing incoming part of stream that doesn't exist");
                    }
                    (None, false) => {
                        log::error!("{id}, closing outgoing part of stream that doesn't exist");
                    }
                }
                let header = if id.incoming == stream_id.initiator_is_incoming { 4 } else { 3 };
                let b = ((stream_id.i << 3) + header).to_be_bytes();
                db_stream.add(id.incoming, id.metadata.time, &b)?;
            }
            Tag::Reset => {
                log::warn!(
                    "{id}, stream {} {} reset",
                    stream_id.i,
                    stream_id.initiator_is_incoming,
                );
                self.inners.remove(&stream_id);
                let header = if id.incoming == stream_id.initiator_is_incoming { 6 } else { 5 };
                let b = ((stream_id.i << 3) + header).to_be_bytes();
                db_stream.add(id.incoming, id.metadata.time, &b)?;
            }
        };

        *accumulator = accumulator[end..].to_vec();
        Ok(())
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + From<(u64, bool)>,
{
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        let accumulator = if id.incoming {
            &mut self.accumulator_incoming
        } else {
            &mut self.accumulator_outgoing
        };
        accumulator.extend_from_slice(bytes);

        let (header, len, offset) = {
            let (v, remaining) = decode::u64(accumulator).unwrap();
            let header = Header::new(v, id.incoming);

            let (len, remaining) = decode::usize(remaining).unwrap();
            let offset = remaining.as_ptr() as usize - accumulator.as_ptr() as usize;
            (header, len, offset)
        };

        #[allow(clippy::comparison_chain)]
        if offset + len == accumulator.len() {
            // good case, we have all data in one chunk
            self.out(id, cx, db, header, offset..(len + offset))?;
        } else if offset + len <= accumulator.len() {
            // TODO:
            panic!("{} < {}", offset + len, accumulator.len());
        }

        Ok(())
    }
}
