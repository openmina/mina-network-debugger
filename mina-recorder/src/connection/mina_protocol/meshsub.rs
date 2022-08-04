use std::fmt;

use mina_serialization_types::v1::ExternalTransitionV1;
use bin_prot::encodable::BinProtEncodable;

use super::{DirectedId, HandleData, Cx};

#[allow(clippy::derive_partial_eq_without_eq)]
mod gossipsub {
    include!(concat!(env!("OUT_DIR"), "/gossipsub.pb.rs"));
}

pub enum Msg {
    ExternalTransition(Box<ExternalTransitionV1>),
    Other { tag: u8, raw: Vec<u8> },
}

impl fmt::Display for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Msg::ExternalTransition(v) => write!(f, "{v:?}"),
            Msg::Other { tag, raw } => write!(f, "({tag} {})", hex::encode(raw)),
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
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        use prost::{bytes::Bytes, Message};

        let _ = (id, cx);

        let buf = Bytes::from(bytes.to_vec());
        let gossipsub::Rpc {
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
                if data[8] == 0 {
                    let v = ExternalTransitionV1::try_decode_binprot(&data[9..]).unwrap();
                    Event::Publish {
                        topic,
                        msg: Msg::ExternalTransition(Box::new(v)),
                    }
                } else {
                    Event::Publish {
                        topic,
                        msg: Msg::Other {
                            tag: data[8],
                            raw: data[9..].to_vec(),
                        },
                    }
                }
            });
        let control = control.into_iter().map(|_c| Event::Control);
        subscriptions.chain(publish).chain(control).collect()
    }
}

#[cfg(test)]
#[test]
fn meshsub_msg() {
    use prost::{bytes::Bytes, Message as _};

    let buf = Bytes::from(hex::decode(include_str!("meshsub_test.hex")).unwrap());
    let msg = gossipsub::Rpc::decode_length_delimited(buf).unwrap();
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
