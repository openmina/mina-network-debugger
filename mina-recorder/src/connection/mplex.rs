use std::{fmt, collections::BTreeMap, mem, iter::Flatten, ops::Range};

use unsigned_varint::decode;

use super::{HandleData, DynamicProtocol, Cx};

#[derive(Default)]
pub struct State<Inner> {
    accumulating: Vec<u8>,
    inners: BTreeMap<StreamId, Inner>,
}

impl<Inner> DynamicProtocol for State<Inner>
where
    Inner: Default,
{
    fn from_name(name: &str) -> Self {
        assert_eq!(name, "/coda/mplex/1.0.0");
        Self::default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamId {
    pub i: u64,
    pub initiator_is_incoming: bool,
}

pub enum Body<Inner> {
    Nothing,
    NewStream(String),
    Message { initiator: bool, inner: Inner },
    Close { initiator: bool },
    Reset { initiator: bool },
}

impl<Inner> fmt::Display for Body<Inner>
where
    Inner: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Body::Nothing => Ok(()),
            Body::NewStream(name) => write!(f, "new stream: \"{name}\""),
            Body::Message { inner, .. } => inner.fmt(f),
            Body::Close { .. } => write!(f, "close"),
            Body::Reset { .. } => write!(f, "reset"),
        }
    }
}

pub struct Output<Inner> {
    pub stream_id: StreamId,
    pub body: Body<Inner>,
}

impl<Inner> Iterator for Output<Inner>
where
    Inner: Iterator,
{
    type Item = Output<Inner::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        let stream_id = self.stream_id;
        let out = |body| Some(Output { stream_id, body });
        match mem::replace(&mut self.body, Body::Nothing) {
            Body::Nothing => None,
            Body::NewStream(n) => out(Body::NewStream(n)),
            Body::Message {
                initiator,
                mut inner,
            } => {
                let inner_item = inner.next()?;
                self.body = Body::Message { initiator, inner };
                out(Body::Message {
                    initiator,
                    inner: inner_item,
                })
            }
            Body::Close { initiator } => out(Body::Close { initiator }),
            Body::Reset { initiator } => out(Body::Reset { initiator }),
        }
    }
}

impl<Inner> fmt::Display for Output<Inner>
where
    Inner: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Output { stream_id, body } = self;
        let StreamId {
            i,
            initiator_is_incoming,
        } = stream_id;
        let mark = if *initiator_is_incoming { "~" } else { "" };
        write!(f, "stream_id: {mark}{i}, {body}")
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
    Inner: HandleData + Default,
    Inner::Output: IntoIterator,
{
    fn out(
        &mut self,
        incoming: bool,
        cx: &mut Cx,
        header: Header,
        range: Range<usize>,
    ) -> Output<<Inner::Output as IntoIterator>::IntoIter> {
        let bytes = &mut self.accumulating[range];
        let Header {
            tag,
            stream_id,
            initiator,
        } = header;
        let body = match tag {
            Tag::New => Body::NewStream(String::from_utf8(bytes.to_vec()).unwrap()),
            Tag::Msg => {
                let inner = self
                    .inners
                    .entry(stream_id)
                    .or_default()
                    .on_data(incoming, bytes, cx)
                    .into_iter();
                Body::Message { initiator, inner }
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
        Output { stream_id, body }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + Default,
    Inner::Output: IntoIterator,
{
    type Output =
        Flatten<<Vec<Output<<Inner::Output as IntoIterator>::IntoIter>> as IntoIterator>::IntoIter>;

    #[inline(never)]
    fn on_data(&mut self, incoming: bool, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        self.accumulating.extend_from_slice(bytes);

        let (header, len, offset) = {
            let (v, remaining) = decode::u64(&self.accumulating).unwrap();
            let header = Header::new(v, incoming);

            let (len, remaining) = decode::usize(remaining).unwrap();
            let offset = remaining.as_ptr() as usize - self.accumulating.as_ptr() as usize;
            (header, len, offset)
        };

        #[allow(clippy::comparison_chain)]
        if offset + len == self.accumulating.len() {
            // good case, we have all data in one chunk
            let out = self.out(incoming, cx, header, offset..(len + offset));
            self.accumulating.clear();
            vec![out].into_iter().flatten()
        } else if offset + len > self.accumulating.len() {
            vec![].into_iter().flatten()
        } else {
            // TODO:
            panic!("{} < {}", offset + len, self.accumulating.len());
        }
    }
}
