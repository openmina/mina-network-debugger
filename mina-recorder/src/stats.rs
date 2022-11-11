use std::{collections::BTreeMap, time::SystemTime};

use mina_p2p_messages::gossip::GossipNetMessageV2;
use radiation::{Absorb, Emit};
use libp2p_core::PeerId;

use crate::decode::{
    meshsub_stats,
    meshsub::{self, ControlIHave},
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
}

impl StatsState {
    pub fn observe<'a>(
        &'a mut self,
        msg: &[u8],
        incoming: bool,
        time: SystemTime,
    ) -> impl Iterator<Item = meshsub_stats::T> + 'a {
        meshsub::parse_it(msg, false, true)
            .unwrap()
            .filter_map(move |event| match event {
                meshsub::Event::PublishV2 {
                    from,
                    hash,
                    message,
                    ..
                } => {
                    let hash = meshsub_stats::Hash(hash);
                    let producer_id = from?;
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
                            None
                        }
                        _ => None,
                    }
                }
                meshsub::Event::Control { ihave, .. } => {
                    for hash in ihave.iter().map(ControlIHave::hashes).flatten() {
                        if let Some((prev, producer_id, height)) = self.incoming.remove(&hash) {
                            let time = time.duration_since(prev).unwrap();
                            log::info!(
                                "insert {:?}, {:?}, {:?}, {}",
                                hash,
                                time,
                                producer_id,
                                height
                            );
                            return Some(meshsub_stats::T {
                                producer_id,
                                hash,
                                time,
                                height,
                            });
                        }
                    }
                    None
                }
                _ => None,
            })
    }
}
