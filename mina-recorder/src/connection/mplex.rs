use std::{fmt, collections::BTreeMap};

use unsigned_varint::decode;

use super::{DirectedId, HandleData, Cx};

#[derive(Default)]
pub struct State<Inner> {
    inners: BTreeMap<u64, Inner>,
}

pub enum Body<Inner> {
    NewStream(String),
    MessageReceiver(Inner),
    MessageInitiator(Inner),
    CloseReceiver,
    CloseInitiator,
    ResetReceiver,
    ResetInitiator,
    Unknown,
}

impl<Inner> fmt::Display for Body<Inner>
where
    Inner: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Body::NewStream(name) => write!(f, "new stream \"{name}\""),
            Body::MessageReceiver(inner) | Body::MessageInitiator(inner) => write!(f, "{inner}"),
            Body::CloseReceiver | Body::CloseInitiator => write!(f, "close"),
            Body::ResetReceiver | Body::ResetInitiator => write!(f, "reset"),
            Body::Unknown => write!(f, "error"),
        }
    }
}

pub struct Output<Inner> {
    pub stream_id: u64,
    pub body: Body<Inner>,
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
{
    type Output = Output<Inner::Output>;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        let (v, remaining) = decode::u64(bytes).unwrap();
        let stream_id = v >> 3;

        let (len, remaining) = decode::usize(remaining).unwrap();
        let offset = remaining.as_ptr() as usize - bytes.as_ptr() as usize;

        // TODO:
        assert_eq!(offset + len, bytes.len());
        let bytes = &mut bytes[offset..(offset + len)];

        let body = match v & 7 {
            0 => Body::NewStream(String::from_utf8(bytes.to_vec()).unwrap()),
            1 => {
                let protocol = self.inners.entry(stream_id).or_default();
                Body::MessageReceiver(protocol.on_data(id, bytes, cx))
            }
            2 => {
                let protocol = self.inners.entry(stream_id).or_default();
                Body::MessageInitiator(protocol.on_data(id, bytes, cx))
            }
            3 => Body::CloseReceiver,
            4 => Body::CloseInitiator,
            5 => Body::ResetReceiver,
            6 => Body::ResetInitiator,
            7 => Body::Unknown,
            _ => unreachable!(),
        };
        Output { stream_id, body }
    }
}
