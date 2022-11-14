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
    incoming: BTreeMap<meshsub_stats::Hash, (SystemTime, PeerId, u32)>,
    stats: meshsub_stats::T,
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
                            if incoming {
                                let height = block
                                    .header
                                    .protocol_state
                                    .body
                                    .consensus_state
                                    .blockchain_length
                                    .0
                                     .0 as u32;
                                if self.stats.height < height {
                                    self.stats.height = height;
                                    self.incoming.clear();
                                    self.stats.events.clear();
                                }
                                if !self.incoming.contains_key(&hash) {
                                    log::info!(
                                        "insert {:?}, {:?}, {:?}, {}",
                                        hash,
                                        time,
                                        producer_id,
                                        height
                                    );
                                    self.incoming.insert(hash, (time, producer_id, height));
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
                                time,
                                peer,
                            });
                        }
                        _ => (),
                    }
                }
                meshsub::Event::Control { ihave, iwant, .. } => {
                    for hash in ihave.iter().map(ControlIHave::hashes).flatten() {
                        if let Some((_, producer_id, height)) = self.incoming.get(&hash) {
                            let height = *height;
                            let producer_id = producer_id.clone();
                            if height == self.stats.height {
                                let kind = if incoming {
                                    meshsub_stats::Kind::RecvIHave
                                } else {
                                    meshsub_stats::Kind::SendIHave
                                };
                                self.stats.events.push(meshsub_stats::Event {
                                    kind,
                                    producer_id,
                                    hash,
                                    time,
                                    peer,
                                });
                            }
                        }
                    }
                    for hash in iwant.iter().map(ControlIWant::hashes).flatten() {
                        if let Some((_, producer_id, height)) = self.incoming.get(&hash) {
                            let height = *height;
                            let producer_id = producer_id.clone();
                            if height == self.stats.height {
                                let kind = if incoming {
                                    meshsub_stats::Kind::RecvIWant
                                } else {
                                    meshsub_stats::Kind::RecvIWant
                                };
                                self.stats.events.push(meshsub_stats::Event {
                                    kind,
                                    producer_id,
                                    hash,
                                    time,
                                    peer,
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
