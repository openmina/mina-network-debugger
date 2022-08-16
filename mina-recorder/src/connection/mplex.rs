use std::{fmt, collections::BTreeMap, ops::Range};

use unsigned_varint::decode;

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db};

pub struct State<Inner> {
    accumulating: Vec<u8>,
    inners: BTreeMap<StreamId, Inner>,
}

impl<Inner> DynamicProtocol for State<Inner> {
    fn from_name(name: &str, _: u64, _: bool) -> Self {
        assert_eq!(name, "/coda/mplex/1.0.0");
        State {
            accumulating: Vec::default(),
            inners: BTreeMap::default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamId {
    pub i: u64,
    pub initiator_is_incoming: bool,
}

pub enum Body {
    NewStream(String),
    Message { initiator: bool },
    Close { initiator: bool },
    Reset { initiator: bool },
}

impl fmt::Display for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Body::NewStream(name) => write!(f, "new stream: \"{name}\""),
            Body::Message { .. } => write!(f, "message"),
            Body::Close { .. } => write!(f, "close"),
            Body::Reset { .. } => write!(f, "reset"),
        }
    }
}

enum Tag {
    New,
    Msg,
    Close,
    Reset,
}

struct Header {
    tag: Tag,
    stream_id: StreamId,
    initiator: bool,
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
            stream_id: StreamId {
                i: v >> 3,
                initiator_is_incoming: initiator == incoming,
            },
            initiator,
        }
    }
}

impl<Inner> State<Inner>
where
    Inner: HandleData + From<(u64, bool)>,
{
    fn out(&mut self, id: DirectedId, cx: &mut Cx, db: &Db, header: Header, range: Range<usize>) {
        let bytes = &mut self.accumulating[range];
        let Header {
            tag,
            stream_id,
            initiator,
        } = header;
        let body = match tag {
            Tag::New => Body::NewStream(String::from_utf8(bytes.to_vec()).unwrap()),
            Tag::Msg => {
                self.inners
                    .entry(stream_id)
                    .or_insert_with(|| Inner::from((stream_id.i, stream_id.initiator_is_incoming)))
                    .on_data(id.clone(), bytes, cx, db);
                Body::Message { initiator }
            }
            Tag::Close => {
                self.inners.remove(&stream_id);
                Body::Close { initiator }
            }
            Tag::Reset => {
                self.inners.remove(&stream_id);
                Body::Reset { initiator }
            }
        };
        let StreamId {
            i,
            initiator_is_incoming,
        } = stream_id;
        let mark = if initiator_is_incoming { "~" } else { "" };
        log::info!("{id} stream_id: {mark}{i}, {body}")
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + From<(u64, bool)>,
{
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) {
        self.accumulating.extend_from_slice(bytes);

        let (header, len, offset) = {
            let (v, remaining) = decode::u64(&self.accumulating).unwrap();
            let header = Header::new(v, id.incoming);

            let (len, remaining) = decode::usize(remaining).unwrap();
            let offset = remaining.as_ptr() as usize - self.accumulating.as_ptr() as usize;
            (header, len, offset)
        };

        #[allow(clippy::comparison_chain)]
        if offset + len == self.accumulating.len() {
            // good case, we have all data in one chunk
            self.out(id, cx, db, header, offset..(len + offset));
            self.accumulating.clear();
        } else if offset + len <= self.accumulating.len() {
            // TODO:
            panic!("{} < {}", offset + len, self.accumulating.len());
        }
    }
}
