use std::{collections::BTreeMap, time::SystemTime, net::SocketAddr, sync::atomic::AtomicUsize};

use mina_p2p_messages::{gossip::GossipNetMessageV2, v2};
use radiation::{Absorb, Emit};
use libp2p_core::PeerId;

use crate::{
    decode::{
        meshsub_stats::{BlockStat, TxStat, Hash, Event, Signature, Tx, Snark},
        meshsub::{self, ControlIHave, ControlIWant},
        MessageType,
    },
    database::{DbFacade, DbError},
};

#[derive(Default, Absorb, Emit)]
pub struct Stats {
    pub decrypted: AtomicUsize,
    pub failed_to_decrypt: AtomicUsize,
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
    // block
    first: BTreeMap<Hash, Description>,
    block_stat: BlockStat,
    // tx
    txs: BTreeMap<Signature, TxDesc>,
    snarks: BTreeMap<Hash, TxDesc>,
    tx_stat: Option<TxStat>,
}

struct Description {
    time: SystemTime,
    producer_id: PeerId,
    block_height: u32,
    global_slot: u32,
}

struct TxDesc {
    time: SystemTime,
    producer_id: PeerId,
    message_id: u64,
}

impl StatsState {
    #[cfg(test)]
    pub fn observe_w(
        &mut self,
        message_id: u64,
        msg: &[u8],
        incoming: bool,
        time: SystemTime,
        db: &crate::database::DbFacade,
        peer: SocketAddr,
    ) {
        let node_address = "0.0.0.0:0".parse().unwrap();
        let _ = self.observe(message_id, msg, incoming, time, time, peer, node_address);
        let block_stat = self.block_stat();
        db.stats(block_stat.height, node_address, &block_stat)
            .unwrap();
        if let Some(stat) = self.tx_stat() {
            db.stats_tx(self.block_stat.height, &stat).unwrap();
        }
    }

    pub fn observe(
        &mut self,
        message_id: u64,
        msg: &[u8],
        incoming: bool,
        time: SystemTime,
        better_time: SystemTime,
        peer: SocketAddr,
        node_address: SocketAddr,
    ) -> (bool, bool, Vec<Event>) {
        let mut events_for_aggregator = vec![];
        let (sender_addr, receiver_addr) = if incoming {
            (peer, node_address)
        } else {
            (node_address, peer)
        };
        let mut block_stat_updated = false;
        let mut tx_stat_updated = true;
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
                                self.tx_stat = None;
                                block_stat_updated = true;
                            }
                            if self.block_stat.height > block_height {
                                // skip obsolete
                                log::warn!("skip obsolete block {block_height}, {hash:?}");
                                continue;
                            }
                            let latency = if let Some(first) = self.first.get(&hash) {
                                Some(time.duration_since(first.time).unwrap_or_default())
                            } else {
                                // TODO: investigate
                                if !incoming {
                                    log::warn!(
                                        "sending block, did not received it before {}",
                                        message_id
                                    );
                                }
                                let v = Description {
                                    time,
                                    producer_id,
                                    block_height,
                                    global_slot,
                                };
                                self.first.insert(hash, v);

                                let tx_stat = self.tx_stat.get_or_insert_with(|| TxStat {
                                    block_time: time,
                                    block_height,
                                    transactions: vec![],
                                    snarks: vec![],
                                    pending_txs: vec![],
                                    // pending_snarks: vec![],
                                });

                                let it0 = block.body.staged_ledger_diff.diff.0.commands.iter();
                                let it1 = block
                                    .body
                                    .staged_ledger_diff
                                    .diff
                                    .1
                                    .iter()
                                    .flat_map(|x| x.commands.iter());
                                for tx in it0.chain(it1) {
                                    match &tx.data {
                                        v2::MinaBaseUserCommandStableV2::SignedCommand(c) => {
                                            let mut signature = Signature([0; 32], [0; 32]);
                                            signature.0.clone_from_slice(c.signature.0.as_ref());
                                            signature.1.clone_from_slice(c.signature.1.as_ref());
                                            if let Some(tx_desc) = self.txs.remove(&signature) {
                                                tx_stat.transactions.push(Tx {
                                                    producer_id: tx_desc.producer_id,
                                                    time: tx_desc.time,
                                                    command: tx.clone(),
                                                    latency: time
                                                        .duration_since(tx_desc.time)
                                                        .unwrap(),
                                                });
                                                tx_stat_updated = true;
                                            }
                                        }
                                        _ => (),
                                    }
                                }
                                tx_stat.pending_txs =
                                    self.txs.values().map(|v| v.message_id).collect();

                                let it0 =
                                    block.body.staged_ledger_diff.diff.0.completed_works.iter();
                                let it1 = block
                                    .body
                                    .staged_ledger_diff
                                    .diff
                                    .1
                                    .iter()
                                    .flat_map(|x| x.completed_works.iter());
                                for snark in it0.chain(it1) {
                                    let hash = match &snark.proofs {
                                        v2::TransactionSnarkWorkTStableV2Proofs::One(p) => {
                                            p.0.statement.target.ledger.clone()
                                        }
                                        v2::TransactionSnarkWorkTStableV2Proofs::Two((_, p)) => {
                                            p.0.statement.target.ledger.clone()
                                        }
                                    };
                                    let hash = hash.into_inner().0.into();
                                    if let Some(desc) = self.snarks.remove(&hash) {
                                        tx_stat.snarks.push(Snark {
                                            producer_id: desc.producer_id,
                                            time: desc.time,
                                            work: snark.clone(),
                                            latency: time.duration_since(desc.time).unwrap(),
                                        });
                                        tx_stat_updated = true;
                                    }
                                }
                                // tx_stat.pending_snarks =
                                //     self.snarks.values().map(|v| v.message_id).collect();

                                None
                            };
                            let event = Event {
                                producer_id,
                                hash,
                                block_height,
                                global_slot,
                                incoming,
                                message_kind: MessageType::PublishNewState,
                                message_id,
                                time,
                                better_time,
                                latency,
                                sender_addr,
                                receiver_addr,
                            };
                            events_for_aggregator.push(event.clone());
                            self.block_stat.events.push(event);
                            block_stat_updated = true;
                        }
                        GossipNetMessageV2::TransactionPoolDiff(transaction) => {
                            for tx in &transaction.0 {
                                match tx {
                                    v2::MinaBaseUserCommandStableV2::SignedCommand(c) => {
                                        let mut signature = Signature([0; 32], [0; 32]);
                                        signature.0.clone_from_slice(c.signature.0.as_ref());
                                        signature.1.clone_from_slice(c.signature.1.as_ref());
                                        self.txs.entry(signature).or_insert_with(|| TxDesc {
                                            time,
                                            producer_id,
                                            message_id,
                                        });
                                    }
                                    _ => (),
                                }
                            }
                        }
                        GossipNetMessageV2::SnarkPoolDiff(snark) => match snark {
                            v2::NetworkPoolSnarkPoolDiffVersionedStableV2::AddSolvedWork(s) => {
                                let hash = match &s.1.proof {
                                    v2::TransactionSnarkWorkTStableV2Proofs::One(p) => {
                                        p.0.statement.target.ledger.clone()
                                    }
                                    v2::TransactionSnarkWorkTStableV2Proofs::Two((_, p)) => {
                                        p.0.statement.target.ledger.clone()
                                    }
                                };
                                let hash = hash.into_inner().0.into();
                                self.snarks.entry(hash).or_insert_with(|| TxDesc {
                                    time,
                                    producer_id,
                                    message_id,
                                });
                            }
                            v2::NetworkPoolSnarkPoolDiffVersionedStableV2::Empty => (),
                        },
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
                                let event = Event {
                                    producer_id,
                                    hash,
                                    block_height,
                                    global_slot,
                                    incoming,
                                    message_kind,
                                    message_id,
                                    time,
                                    better_time,
                                    latency: time.duration_since(first.time).ok(),
                                    sender_addr,
                                    receiver_addr,
                                };
                                events_for_aggregator.push(event.clone());
                                self.block_stat.events.push(event);
                                block_stat_updated = true;
                            }
                        }
                    }
                }
                meshsub::Event::PublishTestingMessage { from, message, .. } => {
                    let parse_block_height = |message: &str| message.split("slot: ").nth(1)?.parse().ok();
                    if let Some(block_height) = parse_block_height(&message) {
                        let event = Event {
                            producer_id: from,
                            hash: Hash([0; 32]),
                            block_height,
                            global_slot: block_height,
                            incoming,
                            message_kind: MessageType::PublishNewState,
                            message_id,
                            time,
                            better_time,
                            latency: None,
                            sender_addr,
                            receiver_addr,
                        };
                        self.block_stat.events.push(event);
                        block_stat_updated = true;
                    }
                }
                _ => (),
            }
        }

        (block_stat_updated, tx_stat_updated, events_for_aggregator)
    }

    pub fn block_stat(&self) -> BlockStat {
        self.block_stat.clone()
    }

    pub fn tx_stat(&self) -> Option<TxStat> {
        self.tx_stat.clone()
    }
}

pub fn update_block_stats(
    message_id: u64,
    msg: &[u8],
    incoming: bool,
    time: SystemTime,
    better_time: SystemTime,
    peer: SocketAddr,
    node_address: SocketAddr,
    db: &DbFacade,
) -> Result<(), DbError> {
    let (sender_addr, receiver_addr) = if incoming {
        (peer, node_address)
    } else {
        (node_address, peer)
    };
    for event in meshsub::parse_it(msg, false, true).unwrap() {
        match event {
            meshsub::Event::PublishV2 {
                from: Some(producer_id),
                message,
                hash,
                ..
            } => match message.as_ref() {
                GossipNetMessageV2::NewState(block) => {
                    let (block_height, global_slot) = {
                        let consensus_state = &block.header.protocol_state.body.consensus_state;
                        (
                            consensus_state.blockchain_length.0 .0 as u32,
                            consensus_state.global_slot_since_genesis.0 .0 as u32,
                        )
                    };
                    let event = Event {
                        producer_id,
                        hash: Hash(hash),
                        block_height,
                        global_slot,
                        incoming,
                        message_kind: MessageType::PublishNewState,
                        message_id,
                        time,
                        better_time,
                        latency: None,
                        sender_addr,
                        receiver_addr,
                    };
                    db.stats_block_v2(event)?;
                }
                _ => {}
            },
            meshsub::Event::Control {
                ihave: _,
                iwant: _,
                graft: _,
                prune: _,
            } => {}
            meshsub::Event::PublishTestingMessage { from, message, hash, .. } => {
                let parse_block_height = |message: &str| message.split("slot: ").nth(1)?.parse().ok();
                if let Some(block_height) = parse_block_height(&message) {
                    let event = Event {
                        producer_id: from,
                        hash: Hash(hash),
                        block_height,
                        global_slot: block_height,
                        incoming,
                        message_kind: MessageType::PublishNewState,
                        message_id,
                        time,
                        better_time,
                        latency: None,
                        sender_addr,
                        receiver_addr,
                    };
                    db.stats_block_v2(event)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
