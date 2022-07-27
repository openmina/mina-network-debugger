use std::{fmt, collections::BTreeMap, mem};

use unsigned_varint::decode;

use super::{DirectedId, HandleData, Cx};

#[derive(Default)]
pub struct State<Inner> {
    inners: BTreeMap<u64, Inner>,
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
            Body::Message { initiator, inner } => {
                if *initiator {
                    write!(f, "initiator: {inner}")
                } else {
                    write!(f, "responder: {inner}")
                }
            }
            Body::Close { initiator } => {
                if *initiator {
                    write!(f, "close initiator")
                } else {
                    write!(f, "close responder")
                }
            }
            Body::Reset { initiator } => {
                if *initiator {
                    write!(f, "reset initiator")
                } else {
                    write!(f, "reset responder")
                }
            }
        }
    }
}

pub struct Output<Inner> {
    pub stream_id: u64,
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
        write!(f, "stream_id: {stream_id}, body: {body}")
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
        let stream_id = v >> 3;

        let (len, remaining) = decode::usize(remaining).unwrap();
        let offset = remaining.as_ptr() as usize - bytes.as_ptr() as usize;

        // TODO:
        assert_eq!(offset + len, bytes.len());
        let bytes = &mut bytes[offset..(offset + len)];

        let initiator = v % 2 == 0;
        let body = match v & 7 {
            0 => Body::NewStream(String::from_utf8(bytes.to_vec()).unwrap()),
            1 | 2 => {
                let protocol = self.inners.entry(stream_id).or_default();
                Body::Message {
                    initiator,
                    inner: protocol.on_data(id, bytes, cx).into_iter(),
                }
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
