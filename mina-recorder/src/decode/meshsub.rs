use std::io::Cursor;

use mina_p2p_messages::{GossipNetMessageV1, gossip::GossipNetMessageV2};
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
        from: Option<String>,
        seqno: Option<String>,
        signature: Option<String>,
        key: Option<String>,
        topic: String,
        message: Box<GossipNetMessageV1>,
    },
    #[serde(rename = "publish_v2")]
    PublishV2 {
        from: Option<String>,
        seqno: Option<String>,
        signature: Option<String>,
        key: Option<String>,
        topic: String,
        message: Box<GossipNetMessageV2>,
    },
    #[serde(rename = "publish")]
    PublishPreview {
        topic: String,
        message: GossipNetMessagePreview,
    },
    Control {
        ihave: Vec<ControlIHave>,
        iwant: Vec<ControlIWant>,
        graft: Vec<ControlGraft>,
        prune: Vec<ControlPrune>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ControlIHave {
    topic_id: Option<String>,
    message_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ControlIWant {
    message_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ControlGraft {
    topic_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ControlPrune {
    topic_id: Option<String>,
    peers: Vec<PeerInfo>,
    backoff: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PeerInfo {
    peer_id: Option<String>,
    signed_peer_record: Option<String>,
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
        control,
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
        .filter_map(|data| data.get(8).cloned())
        .filter_map(|tag| match tag {
            0 => Some(MessageType::PublishExternalTransition),
            1 => Some(MessageType::PublishSnarkPoolDiff),
            2 => Some(MessageType::PublishTransactionPoolDiff),
            _ => None,
        });
    let control = control
        .into_iter()
        .filter(|c| {
            !(c.ihave.is_empty() && c.iwant.is_empty() && c.graft.is_empty() && c.prune.is_empty())
        })
        .map(|_| MessageType::Control);

    Ok(subscriptions.chain(publish).chain(control).collect())
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
        .filter_map(|msg| {
            msg.data
                .map(|d| (d, msg.topic, msg.from, msg.seqno, msg.signature, msg.key))
        })
        .map(|(data, topic, from, seqno, signature, key)| {
            let mut c = Cursor::new(&data[8..]);
            match GossipNetMessageV1::binprot_read(&mut c) {
                Ok(msg) => {
                    let message = Box::new(msg);
                    if preview {
                        let message = match &*message {
                            GossipNetMessageV1::NewState(_) => GossipNetMessagePreview::NewState,
                            GossipNetMessageV1::SnarkPoolDiff(_) => {
                                GossipNetMessagePreview::SnarkPoolDiff
                            }
                            GossipNetMessageV1::TransactionPoolDiff(_) => {
                                GossipNetMessagePreview::TransactionPoolDiff
                            }
                        };
                        Event::PublishPreview { topic, message }
                    } else {
                        Event::Publish {
                            from: from.map(hex::encode),
                            seqno: seqno.map(hex::encode),
                            signature: signature.map(hex::encode),
                            key: key.map(hex::encode),
                            topic,
                            message,
                        }
                    }
                }
                Err(_) => {
                    let mut c = Cursor::new(&data[8..]);
                    let msg = GossipNetMessageV2::binprot_read(&mut c).unwrap();
                    let message = Box::new(msg);
                    if preview {
                        let message = match &*message {
                            GossipNetMessageV2::NewState(_) => GossipNetMessagePreview::NewState,
                            GossipNetMessageV2::SnarkPoolDiff(_) => {
                                GossipNetMessagePreview::SnarkPoolDiff
                            }
                            GossipNetMessageV2::TransactionPoolDiff(_) => {
                                GossipNetMessagePreview::TransactionPoolDiff
                            }
                        };
                        Event::PublishPreview { topic, message }
                    } else {
                        Event::PublishV2 {
                            from: from.map(hex::encode),
                            seqno: seqno.map(hex::encode),
                            signature: signature.map(hex::encode),
                            key: key.map(hex::encode),
                            topic,
                            message,
                        }
                    }
                }
            }
        });
    let control = control.into_iter().map(
        |pb::ControlMessage {
             ihave,
             iwant,
             graft,
             prune,
         }| Event::Control {
            ihave: ihave
                .into_iter()
                .map(|m| ControlIHave {
                    topic_id: m.topic_id,
                    message_ids: m.message_ids.into_iter().map(hex::encode).collect(),
                })
                .collect(),
            iwant: iwant
                .into_iter()
                .map(|m| ControlIWant {
                    message_ids: m.message_ids.into_iter().map(hex::encode).collect(),
                })
                .collect(),
            graft: graft
                .into_iter()
                .map(|m| ControlGraft {
                    topic_id: m.topic_id,
                })
                .collect(),
            prune: prune
                .into_iter()
                .map(|m| ControlPrune {
                    topic_id: m.topic_id,
                    peers: m
                        .peers
                        .into_iter()
                        .map(|peer| PeerInfo {
                            peer_id: peer.peer_id.map(hex::encode),
                            signed_peer_record: peer.signed_peer_record.map(hex::encode),
                        })
                        .collect(),
                    backoff: m.backoff,
                })
                .collect(),
        },
    );
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

    use mina_p2p_messages::v1::MinaBlockExternalTransitionRawVersionedStableV1Versioned as Msg;

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
