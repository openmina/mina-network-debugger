use std::{io, net::SocketAddr, time::SystemTime, collections::BTreeMap};

use binprot::BinProtRead;
use mina_p2p_messages::gossip::GossipNetMessageV2;
use radiation::{Absorb, Emit};

use crate::database::{DbCore, CapnpEventWithMetadataKey, CapnpEventWithMetadata};

#[derive(Default)]
pub struct CapnpReader {
    buffer: Vec<u8>,
}

#[derive(Absorb, Emit)]
pub enum CapnpEvent {
    ReceivedGossip {
        peer_id: String,
        peer_host: String,
        peer_port: u16,
        msg: Vec<u8>,
        hash: [u8; 32],
    },
    Publish {
        msg: Vec<u8>,
        hash: [u8; 32],
    },
}

impl CapnpReader {
    pub fn extend_from_slice(&mut self, other: &[u8]) {
        self.buffer.extend_from_slice(other);
    }

    pub fn process(
        &mut self,
        pid: u32,
        incoming: bool,
        node_address: SocketAddr,
        time: SystemTime,
        real_time: SystemTime,
        db: &DbCore,
        subscriptions: &mut BTreeMap<u64, String>,
        chain_id: &mut String,
    ) -> bool {
        let mut events = vec![];
        let should_continue = loop {
            if !self.buffer.is_empty() {
                let mut slice = self.buffer.as_slice();

                let r = if incoming {
                    process_request(pid, "<-", &mut slice, &mut events, subscriptions, chain_id)
                } else {
                    process_response(pid, "->", &mut slice, &mut events, subscriptions)
                };
                match r {
                    Ok(()) => {
                        log::debug!(
                            "capnp {pid} {incoming} consumed: {}",
                            self.buffer.len() - slice.len()
                        );
                        self.buffer = slice.to_vec();
                    }
                    Err(err) if err.description == "failed to fill the whole buffer" => {
                        log::debug!("capnp {pid} {incoming} waiting more data");
                        break true;
                    }
                    Err(err) => {
                        let s0 = err.description.starts_with("Too many segments:");
                        let s1 = err.description.starts_with("Too few segments:");
                        if !(s0 || s1) {
                            log::error!("capnp {pid} {incoming} {err} {}", hex::encode(slice));
                        }
                        break false;
                    }
                }
            } else {
                break true;
            }
        };

        if should_continue && !events.is_empty() {
            let height = events.iter().find_map(|e| match e {
                CapnpEvent::Publish { msg, .. } | CapnpEvent::ReceivedGossip { msg, .. } => {
                    if msg[0] == 0 {
                        let mut slice = msg.as_slice();
                        match GossipNetMessageV2::binprot_read(&mut slice) {
                            Ok(GossipNetMessageV2::NewState(block)) => {
                                let height = block
                                    .header
                                    .protocol_state
                                    .body
                                    .consensus_state
                                    .blockchain_length
                                    .0
                                     .0 as u32;
                                Some(height)
                            }
                            Ok(_) => None,
                            Err(err) => {
                                log::error!("decode binprot from IPC {err}");
                                None
                            }
                        }
                    } else if msg[0] == 3 {
                        let bytes = msg[1..].to_vec();
                        let parse_block_height = |bytes| String::from_utf8(bytes).ok()?.split("slot: ").nth(1)?.parse().ok();
                        parse_block_height(bytes)
                    } else {
                        None
                    }
                }
            });
            if let Some(height) = height {
                let key = CapnpEventWithMetadataKey { height, time };
                let value = CapnpEventWithMetadata {
                    real_time,
                    node_address,
                    events,
                };
                db.put_capnp(key, value).unwrap();
            }
        }

        should_continue
    }
}

fn calc_hash(data: &[u8], topic: &str) -> [u8; 32] {
    use blake2::digest::{Mac, Update, FixedOutput, typenum};

    let key;
    let key = if topic.as_bytes().len() <= 64 {
        topic.as_bytes()
    } else {
        key = blake2::Blake2b::<typenum::U32>::default()
            .chain(topic.as_bytes())
            .finalize_fixed();
        key.as_slice()
    };
    blake2::Blake2bMac::<typenum::U32>::new_from_slice(key)
        .unwrap()
        .chain(data)
        .finalize_fixed()
        .into()
}

// TODO: figure out how to capture this, doesn't work for now
pub fn process_request<R>(
    pid: u32,
    incoming: &str,
    reader: R,
    events: &mut Vec<CapnpEvent>,
    subscriptions: &mut BTreeMap<u64, String>,
    chain_id: &mut String,
) -> capnp::Result<()>
where
    R: io::Read,
{
    use capnp::serialize;
    use crate::libp2p_ipc_capnp::libp2p_helper_interface::{message, rpc_request, push_message};

    let reader = serialize::read_message(reader, Default::default())?;

    let t = reader.get_root::<message::Reader>()?;

    match t.which()? {
        message::RpcRequest(Ok(msg)) => match msg.which() {
            Ok(rpc_request::Configure(Ok(config))) => {
                let network_id = config.get_config()?.get_network_id()?;
                *chain_id = format!("/coda/0.0.1/{network_id}");
            }
            Ok(rpc_request::AddPeer(Ok(peer))) => {
                let addr = peer.get_multiaddr().unwrap().get_representation().unwrap();
                log::debug!("capnp message {pid} {incoming} add_peer {addr}");
            }
            Ok(rpc_request::Publish(Ok(msg))) => {
                let topic = msg.get_topic().unwrap();
                // log::info!("capnp message {pid} {incoming} publish {topic}");

                let data = msg.get_data().unwrap();
                events.push(CapnpEvent::Publish {
                    msg: data[8..].to_vec(),
                    hash: calc_hash(data, topic),
                });
            }
            Ok(rpc_request::OpenStream(Ok(stream))) => {
                let peer = stream.get_peer().unwrap().get_id().unwrap();
                let protocol = stream.get_protocol_id().unwrap();
                log::debug!("capnp message {pid} {incoming} open stream {peer} {protocol}");
            }
            Ok(rpc_request::CloseStream(Ok(stream))) => {
                let id = stream.get_stream_id().unwrap().get_id();
                log::debug!("capnp message {pid} {incoming} close stream {id}");
            }
            Ok(rpc_request::ResetStream(Ok(stream))) => {
                let id = stream.get_stream_id().unwrap().get_id();
                log::debug!("capnp message {pid} {incoming} reset stream {id}");
            }
            Ok(rpc_request::SendStream(Ok(msg))) => {
                let msg = msg.get_msg().unwrap();
                let id = msg.get_stream_id().unwrap().get_id();
                let data = msg.get_data().unwrap();
                log::debug!(
                    "capnp message {pid} {incoming} send stream {id} data size: {}",
                    data.len()
                );
            }
            Ok(rpc_request::Subscribe(Ok(x))) => {
                if let (Ok(id), Ok(topic)) = (x.get_subscription_id(), x.get_topic()) {
                    subscriptions.insert(id.get_id(), topic.to_owned());
                }
            }
            _ => (),
        },
        message::PushMessage(Ok(msg)) => match msg.which() {
            Ok(push_message::AddResource(Ok(resource))) => {
                let _ = resource;
            }
            _ => (),
        },
        _ => (),
    }

    Ok(())
}

pub fn process_response<R>(
    pid: u32,
    incoming: &str,
    reader: R,
    events: &mut Vec<CapnpEvent>,
    subscriptions: &mut BTreeMap<u64, String>,
) -> capnp::Result<()>
where
    R: io::Read,
{
    use capnp::serialize;
    use crate::libp2p_ipc_capnp::{
        daemon_interface::{message, push_message},
        libp2p_helper_interface::{rpc_response, rpc_response_success},
    };

    let reader = serialize::read_message(reader, Default::default())?;

    let t = reader.get_root::<message::Reader>()?;

    match t.which()? {
        message::PushMessage(Ok(msg)) => match msg.which() {
            Ok(push_message::PeerConnected(Ok(peer))) => {
                let id = peer.get_peer_id().unwrap().get_id().unwrap();
                log::info!("capnp message {pid} {incoming} connected {id}");
            }
            Ok(push_message::PeerDisconnected(Ok(peer))) => {
                let id = peer.get_peer_id().unwrap().get_id().unwrap();
                log::info!("capnp message {pid} {incoming} disconnected {id}");
            }
            Ok(push_message::IncomingStream(Ok(stream))) => {
                let peer = stream
                    .get_peer()
                    .unwrap()
                    .get_peer_id()
                    .unwrap()
                    .get_id()
                    .unwrap();
                let protocol = stream.get_protocol().unwrap();
                let id = stream.get_stream_id().unwrap().get_id();
                log::debug!("capnp message {pid} {incoming} open stream {peer} {protocol} {id}");
            }
            Ok(push_message::StreamMessageReceived(Ok(msg))) => {
                let msg = msg.get_msg().unwrap();
                let id = msg.get_stream_id().unwrap().get_id();
                let data = msg.get_data().unwrap();
                log::debug!("capnp message {pid} {incoming} msg {id} {}", data.len());
            }
            Ok(push_message::GossipReceived(Ok(msg))) => {
                let sender = msg.get_sender().unwrap();
                let peer_id = sender.get_peer_id().unwrap().get_id().unwrap().to_owned();
                let peer_host = sender.get_host().unwrap().to_owned();
                let peer_port = sender.get_libp2p_port();

                let data = msg.get_data().unwrap();
                let x = msg.get_subscription_id().unwrap().get_id();

                let topic = subscriptions
                    .get(&x)
                    .cloned()
                    .unwrap_or("coda/consensus-messages/0.0.1".to_owned());

                events.push(CapnpEvent::ReceivedGossip {
                    peer_id,
                    peer_host,
                    peer_port,
                    msg: data[8..].to_vec(),
                    hash: calc_hash(data, &topic),
                });
            }
            _ => (),
        },
        message::RpcResponse(Ok(response)) => match response.which() {
            Ok(rpc_response::Success(Ok(response))) => match response.which() {
                Ok(rpc_response_success::Listen(Ok(addresses))) => {
                    for addr in addresses.get_result().unwrap() {
                        let addr = addr.get_representation().unwrap();
                        log::debug!("capnp message {pid} {incoming} listen {addr}");
                    }
                }
                _ => (),
            },
            _ => (),
        },
        _ => (),
    }

    Ok(())
}
