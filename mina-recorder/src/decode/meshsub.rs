use std::io::Cursor;

use mina_p2p_messages::GossipNetMessage;
use binprot::BinProtRead;
use serde::Serialize;
use prost::{bytes::Bytes, Message};

use super::{DecodeError, MessageType};

#[allow(clippy::derive_partial_eq_without_eq)]
mod pb {
    include!(concat!(env!("OUT_DIR"), "/gossipsub.pb.rs"));
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum Event {
    Subscribe {
        topic: String,
    },
    Unsubscribe {
        topic: String,
    },
    Publish {
        topic: String,
        message: Box<GossipNetMessage>,
    },
    #[serde(rename = "publish")]
    PublishPreview {
        topic: String,
        message: GossipNetMessagePreview,
    },
    Control,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "message")]
pub enum GossipNetMessagePreview {
    #[serde(rename = "external_transition")]
    NewState,
    #[serde(rename = "snark_pool_diff")]
    SnarkPoolDiff,
    #[serde(rename = "transaction_pool_diff")]
    TransactionPoolDiff,
}

pub fn parse_types(bytes: &[u8]) -> Result<Vec<MessageType>, DecodeError> {
    let buf = Bytes::from(bytes.to_vec());
    let pb::Rpc {
        subscriptions,
        publish,
        control: _,
    } = Message::decode_length_delimited(buf).map_err(DecodeError::Protobuf)?;
    let subscriptions = subscriptions.into_iter().map(|v| {
        if v.subscribe() {
            MessageType::Subscribe
        } else {
            MessageType::Unsubscribe
        }
    });
    let publish = publish
        .into_iter()
        .filter_map(|msg| msg.data)
        .filter_map(|data| data.get(9).cloned())
        .filter_map(|tag| match tag {
            0 => Some(MessageType::PublishExternalTransition),
            1 => Some(MessageType::PublishSnarkPoolDiff),
            2 => Some(MessageType::PublishTransactionPoolDiff),
            _ => None,
        });

    Ok(subscriptions.chain(publish).collect())
}

pub fn parse(bytes: Vec<u8>, preview: bool) -> Result<serde_json::Value, DecodeError> {
    let buf = Bytes::from(bytes);
    let pb::Rpc {
        subscriptions,
        publish,
        control,
    } = Message::decode_length_delimited(buf).map_err(DecodeError::Protobuf)?;
    let subscriptions = subscriptions.into_iter().map(|v| {
        let subscribe = v.subscribe();
        let topic = v.topic_id.unwrap_or_default();
        if subscribe {
            Event::Subscribe { topic }
        } else {
            Event::Unsubscribe { topic }
        }
    });
    let publish = publish
        .into_iter()
        .filter_map(|msg| msg.data.map(|d| (d, msg.topic)))
        .map(|(data, topic)| {
            let mut c = Cursor::new(&data[8..]);
            let message = Box::new(GossipNetMessage::binprot_read(&mut c).unwrap());
            if preview {
                let message = match &*message {
                    GossipNetMessage::NewState(_) => GossipNetMessagePreview::NewState,
                    GossipNetMessage::SnarkPoolDiff(_) => GossipNetMessagePreview::SnarkPoolDiff,
                    GossipNetMessage::TransactionPoolDiff(_) => {
                        GossipNetMessagePreview::TransactionPoolDiff
                    }
                };
                Event::PublishPreview { topic, message }
            } else {
                Event::Publish { topic, message }
            }
        });
    let control = control.into_iter().map(|_c| Event::Control);
    let t = subscriptions
        .chain(publish)
        .chain(control)
        .collect::<Vec<_>>();
    serde_json::to_value(&t).map_err(DecodeError::Serde)
}

#[cfg(test)]
#[test]
fn tag0_msg() {
    use std::io::Cursor;

    use mina_p2p_messages::p2p::MinaBlockExternalTransitionRawVersionedStable as Msg;

    use prost::{bytes::Bytes, Message as _};

    let buf = Bytes::from(hex::decode(include_str!("tag_0.hex")).expect("test"));
    let msg = pb::Rpc::decode_length_delimited(buf).expect("test");
    for a in msg
        .publish
        .into_iter()
        .filter_map(|msg| msg.data)
        .map(|data| {
            if data[8] == 0 {
                let mut c = Cursor::new(&data[9..]);
                Some(Msg::binprot_read(&mut c).expect("test"))
            } else {
                None
            }
        })
    {
        println!("{a:?}");
    }
}
