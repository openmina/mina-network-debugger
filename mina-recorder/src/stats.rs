use std::{collections::BTreeMap, time::SystemTime, net::SocketAddr};

use mina_p2p_messages::gossip::GossipNetMessageV2;
use radiation::{Absorb, Emit};
use libp2p_core::PeerId;

use super::database::DbFacade;

use crate::decode::{
    meshsub_stats::{BlockStat, Hash, Event},
    meshsub::{self, ControlIHave, ControlIWant},
    MessageType,
};

#[derive(Default, Absorb, Emit)]
pub struct Stats {
    pub decrypted: usize,
    pub failed_to_decrypt: usize,
}

// #[derive(Default, Clone, Absorb, Emit, Serialize)]
// pub struct OverallStats {
//     pub connections: u32,
//     pub buffered: u32,
//     pub total_bytes: u64,
//     pub decrypted_bytes: u64,
//     pub chunks: u64,
//     pub messages: u64,
// }

#[derive(Default)]
pub struct StatsState {
    first: BTreeMap<Hash, Description>,
    block_stat: BlockStat,
}

struct Description {
    time: SystemTime,
    producer_id: PeerId,
    block_height: u32,
    global_slot: u32,
}

impl StatsState {
    pub fn observe<'a>(
        &'a mut self,
        msg: &[u8],
        incoming: bool,
        time: SystemTime,
        db: &DbFacade,
        peer: SocketAddr,
    ) {
        let (sender_addr, receiver_addr) = if incoming {
            (peer.to_string(), "local node".to_string())
        } else {
            ("local node".to_string(), peer.to_string())
        };
        let message_id = db.next_message_id();
        for event in meshsub::parse_it(msg, false, true).unwrap() {
            match event {
                meshsub::Event::PublishV2 {
                    from: Some(producer_id),
                    hash,
                    message,
                    ..
                } => {
                    let hash = Hash(hash);
                    match message.as_ref() {
                        GossipNetMessageV2::NewState(block) => {
                            let block_height = block
                                .header
                                .protocol_state
                                .body
                                .consensus_state
                                .blockchain_length
                                .0
                                 .0 as u32;
                            let global_slot = block
                                .header
                                .protocol_state
                                .body
                                .consensus_state
                                .global_slot_since_genesis
                                .0
                                 .0 as u32;
                            if self.block_stat.height < block_height {
                                self.first.clear();
                                self.block_stat.clear();
                                self.block_stat.height = block_height;
                            }
                            if self.block_stat.height > block_height {
                                // skip obsolete
                                continue;
                            }
                            let latency = if let Some(first) = self.first.get(&hash) {
                                Some(time.duration_since(first.time).unwrap_or_default())
                            } else {
                                // TODO: investigate
                                if !incoming {
                                    log::warn!("sending block, did not received it before {}", message_id);
                                } else {
                                    let v = Description { time, producer_id, block_height, global_slot };
                                    self.first.insert(hash, v);
                                }

                                None
                            };
                            self.block_stat.events.push(Event {
                                producer_id,
                                hash,
                                block_height,
                                global_slot,
                                incoming,
                                message_kind: MessageType::PublishNewState,
                                message_id,
                                time,
                                latency,
                                sender_addr: sender_addr.clone(),
                                receiver_addr: receiver_addr.clone(),
                            });
                        }
                        GossipNetMessageV2::SnarkPoolDiff(snark) => {
                            let _ = snark;
                            // TODO:
                        }
                        GossipNetMessageV2::TransactionPoolDiff(transaction) => {
                            let _ = transaction;
                            // TODO:
                        }
                    }
                }
                meshsub::Event::Control { ihave, iwant, .. } => {
                    let h = ihave
                        .iter()
                        .flat_map(ControlIHave::hashes)
                        .map(|hash| (hash, MessageType::ControlIHave));
                    let w = iwant
                        .iter()
                        .flat_map(ControlIWant::hashes)
                        .map(|hash| (hash, MessageType::ControlIWant));
                    for (hash, message_kind) in h.chain(w) {
                        if let Some(first) = self.first.get(&hash) {
                            let block_height = first.block_height;
                            let global_slot = first.global_slot;
                            let producer_id = first.producer_id;
                            if block_height == self.block_stat.height {
                                self.block_stat.events.push(Event {
                                    producer_id,
                                    hash,
                                    block_height,
                                    global_slot,
                                    incoming,
                                    message_kind,
                                    message_id,
                                    time,
                                    latency: Some(time.duration_since(first.time).unwrap_or_default()),
                                    sender_addr: sender_addr.clone(),
                                    receiver_addr: receiver_addr.clone(),
                                });
                            }
                        }
                    }
                }
                _ => (),
            }
        }
        db.stats(self.block_stat.height, &self.block_stat).unwrap();
    }
}
