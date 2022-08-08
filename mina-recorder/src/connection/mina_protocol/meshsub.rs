use std::fmt;

use mina_serialization_types::v1::ExternalTransitionV1;
use bin_prot::encodable::BinProtEncodable;

use super::{HandleData, Cx};

#[allow(clippy::derive_partial_eq_without_eq)]
mod pb {
    include!(concat!(env!("OUT_DIR"), "/gossipsub.pb.rs"));
}

pub enum Msg {
    Transition(Box<ExternalTransitionV1>),
    TransactionsPoolDiff(Vec<u8>),
    SnarkPoolDiff(Vec<u8>),
    Unrecognized(u8, Vec<u8>),
}

impl fmt::Display for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Msg::Transition(v) => write!(f, "Transition({v:?})"),
            Msg::TransactionsPoolDiff(v) => write!(f, "TransactionsPoolDiff({})", hex::encode(v)),
            Msg::SnarkPoolDiff(v) => write!(f, "SnarkPoolDiff({})", hex::encode(v)),
            Msg::Unrecognized(tag, v) => write!(f, "Tag{tag}({})", hex::encode(v)),
        }
    }
}

pub enum Event {
    Subscribe(String),
    Unsubscribe(String),
    Publish { topic: String, msg: Msg },
    Control,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Subscribe(topic) => write!(f, "subscribe {topic}"),
            Event::Unsubscribe(topic) => write!(f, "unsubscribe {topic}"),
            Event::Publish { topic, msg } => write!(f, "publish {topic} {msg}"),
            Event::Control => write!(f, "control message, unimplemented"),
        }
    }
}

#[derive(Default)]
pub struct State {}

impl HandleData for State {
    type Output = Vec<Event>;

    #[inline(never)]
    fn on_data(&mut self, incoming: bool, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        use prost::{bytes::Bytes, Message};

        let _ = (incoming, cx);

        let buf = Bytes::from(bytes.to_vec());
        let pb::Rpc {
            subscriptions,
            publish,
            control,
        } = Message::decode_length_delimited(buf).unwrap();

        let subscriptions = subscriptions.into_iter().map(|v| {
            let subscribe = v.subscribe();
            let topic = v.topic_id.unwrap_or_default();
            if subscribe {
                Event::Subscribe(topic)
            } else {
                Event::Unsubscribe(topic)
            }
        });
        let publish = publish
            .into_iter()
            .filter_map(|msg| msg.data.map(|d| (d, msg.topic)))
            .map(|(data, topic)| {
                let msg = match data[8] {
                    0 => {
                        let v = ExternalTransitionV1::try_decode_binprot(&data[9..]).unwrap();
                        Msg::Transition(Box::new(v))
                    }
                    1 => Msg::TransactionsPoolDiff(data[9..].to_vec()),
                    2 => Msg::SnarkPoolDiff(data[9..].to_vec()),
                    tag => Msg::Unrecognized(tag, data[9..].to_vec()),
                };
                Event::Publish { topic, msg }
            });
        let control = control.into_iter().map(|_c| Event::Control);
        subscriptions.chain(publish).chain(control).collect()
    }
}

#[cfg(test)]
#[test]
fn tag0_msg() {
    use prost::{bytes::Bytes, Message as _};

    let buf = Bytes::from(hex::decode(include_str!("tag_0.hex")).unwrap());
    let msg = pb::Rpc::decode_length_delimited(buf).unwrap();
    for a in msg
        .publish
        .into_iter()
        .filter_map(|msg| msg.data)
        .map(|data| {
            if data[8] == 0 {
                Some(ExternalTransitionV1::try_decode_binprot(&data[9..]).unwrap())
            } else {
                None
            }
        })
    {
        println!("{a:?}");
    }
}
