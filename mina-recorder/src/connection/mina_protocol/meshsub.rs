use mina_serialization_types::v1::ExternalTransitionV1;
use bin_prot::encodable::BinProtEncodable;
use serde::Serialize;

#[allow(clippy::derive_partial_eq_without_eq)]
mod pb {
    include!(concat!(env!("OUT_DIR"), "/gossipsub.pb.rs"));
}

pub fn parse(bytes: Vec<u8>) -> impl Serialize {
    #[derive(Serialize)]
    pub enum Event {
        Subscribe(String),
        Unsubscribe(String),
        Publish { topic: String, msg: Msg },
        Control,
    }

    #[derive(Serialize)]
    pub enum Msg {
        Transition(Box<ExternalTransitionV1>),
        TransactionsPoolDiff(String),
        SnarkPoolDiff(String),
        Unrecognized { tag: u8, hex: String },
    }

    use prost::{bytes::Bytes, Message};

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
                1 => Msg::TransactionsPoolDiff(hex::encode(&data[9..])),
                2 => Msg::SnarkPoolDiff(hex::encode(&data[9..])),
                tag => Msg::Unrecognized {
                    tag,
                    hex: hex::encode(&data[9..]),
                },
            };
            Event::Publish { topic, msg }
        });
    let control = control.into_iter().map(|_c| Event::Control);
    subscriptions
        .chain(publish)
        .chain(control)
        .collect::<Vec<_>>()
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
