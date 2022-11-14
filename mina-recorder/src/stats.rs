use std::{collections::BTreeMap, time::SystemTime, net::SocketAddr};

use mina_p2p_messages::gossip::GossipNetMessageV2;
use radiation::{Absorb, Emit};
use libp2p_core::PeerId;

use super::database::DbFacade;

use crate::decode::{
    meshsub_stats,
    meshsub::{self, ControlIHave, ControlIWant},
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
    incoming: BTreeMap<meshsub_stats::Hash, (PeerId, u32, u32)>,
    stats: meshsub_stats::T,
    block_height: u32,
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
                    let hash = meshsub_stats::Hash(hash);
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
                            if incoming {
                                if self.block_height < block_height {
                                    self.block_height = block_height;
                                    self.incoming.clear();
                                    self.stats.events.clear();
                                }
                                if !self.incoming.contains_key(&hash) {
                                    let v = (producer_id, block_height, global_slot);
                                    self.incoming.insert(hash, v);
                                }
                            }
                            let kind = if incoming {
                                meshsub_stats::Kind::RecvValue
                            } else {
                                meshsub_stats::Kind::SendValue
                            };
                            self.stats.events.push(meshsub_stats::Event {
                                kind,
                                producer_id,
                                hash,
                                message_id,
                                block_height,
                                global_slot,
                                time,
                                sender_addr: sender_addr.clone(),
                                receiver_addr: receiver_addr.clone(),
                            });
                        }
                        _ => (),
                    }
                }
                meshsub::Event::Control { ihave, iwant, .. } => {
                    let h = ihave.iter().map(ControlIHave::hashes).flatten().map(|hash| {
                        let kind = if incoming {
                            meshsub_stats::Kind::RecvIHave
                        } else {
                            meshsub_stats::Kind::SendIHave
                        };
                        (hash, kind)
                    });
                    let w = iwant.iter().map(ControlIWant::hashes).flatten().map(|hash| {
                        let kind = if incoming {
                            meshsub_stats::Kind::RecvIWant
                        } else {
                            meshsub_stats::Kind::SendIWant
                        };
                        (hash, kind)
                    });
                    for (hash, kind) in h.chain(w) {
                        if let Some((producer_id, block_height, global_slot)) = self.incoming.get(&hash) {
                            let block_height = *block_height;
                            let global_slot = *global_slot;
                            let producer_id = producer_id.clone();
                            if block_height == self.block_height {
                                self.stats.events.push(meshsub_stats::Event {
                                    kind,
                                    producer_id,
                                    hash,
                                    message_id,
                                    block_height,
                                    global_slot,
                                    time,
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
        db.stats(self.stats.height, self.stats.clone()).unwrap();
    }
}
