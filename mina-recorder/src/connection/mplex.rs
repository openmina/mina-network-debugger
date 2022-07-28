use std::{fmt, collections::BTreeMap, mem};

use unsigned_varint::decode;

use super::{DirectedId, HandleData, DynamicProtocol, Cx};

#[derive(Default)]
pub struct State<Inner> {
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

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + Default,
    Inner::Output: IntoIterator,
{
    type Output = Output<<Inner::Output as IntoIterator>::IntoIter>;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        let (v, remaining) = decode::u64(bytes).unwrap();
        let i = v >> 3;
        let initiator = v % 2 == 0;
        let initiator_is_incoming = initiator == id.incoming;
        let stream_id = StreamId {
            i,
            initiator_is_incoming,
        };

        let (len, remaining) = decode::usize(remaining).unwrap();
        let offset = remaining.as_ptr() as usize - bytes.as_ptr() as usize;

        // TODO:
        assert_eq!(offset + len, bytes.len());
        let bytes = &mut bytes[offset..(offset + len)];

        let body = match v & 7 {
            0 => Body::NewStream(String::from_utf8(bytes.to_vec()).unwrap()),
            1 | 2 => {
                let inner = self
                    .inners
                    .entry(stream_id)
                    .or_default()
                    .on_data(id, bytes, cx)
                    .into_iter();
                Body::Message { initiator, inner }
            }
            3 | 4 => {
                self.inners.remove(&stream_id);
                Body::Close { initiator }
            }
            5 | 6 => {
                self.inners.remove(&stream_id);
                Body::Reset { initiator }
            }
            7 => panic!(),
            _ => unreachable!(),
        };
        Output { stream_id, body }
    }
}
