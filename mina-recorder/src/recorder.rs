use std::collections::{BTreeMap, VecDeque};

use super::{
    EventMetadata, ConnectionId, DirectedId,
    connection::{HandleData, pnet, multistream_select, noise, mplex, mina_protocol},
};

type Cn = pnet::State<Noise>;
type Noise = multistream_select::State<noise::State<Encrypted>>;
type Encrypted = multistream_select::State<mplex::State<Inner>>;
type Inner = multistream_select::State<mina_protocol::State>;

#[derive(Default)]
pub struct P2pRecorder {
    cns: BTreeMap<ConnectionId, Cn>,
    cx: Cx,
}

#[derive(Default)]
pub struct Cx {
    randomness: VecDeque<[u8; 32]>,
}

impl Cx {
    pub fn push_randomness(&mut self, bytes: [u8; 32]) {
        self.randomness.push_back(bytes);
    }

    pub fn iter_rand(&self) -> impl Iterator<Item = &[u8; 32]> + '_ {
        self.randomness.iter().rev()
    }
}

impl P2pRecorder {
    pub fn on_connect(&mut self, incoming: bool, metadata: EventMetadata) {
        let ConnectionId {
            alias,
            addr,
            pid,
            fd,
        } = &metadata.id;
        if incoming {
            log::info!("{alias}_{pid} accept {addr} {fd}");
        } else {
            log::info!("{alias}_{pid} connect {addr} {fd}");
        }
        self.cns.insert(metadata.id, Default::default());
    }

    pub fn on_disconnect(&mut self, metadata: EventMetadata) {
        let ConnectionId {
            alias,
            addr,
            pid,
            fd,
        } = &metadata.id;
        log::info!("{alias}_{pid} disconnect {addr} {fd}");
        self.cns.remove(&metadata.id);
    }

    pub fn on_data(&mut self, incoming: bool, metadata: EventMetadata, mut bytes: Vec<u8>) {
        if let Some(cn) = self.cns.get_mut(&metadata.id) {
            let id = DirectedId { metadata, incoming };
            let output = cn.on_data(id.clone(), &mut bytes, &mut self.cx);
            for item in output {
                log::info!("{id} {item}");
            }
        }
    }

    pub fn on_randomness(&mut self, _alias: String, bytes: [u8; 32]) {
        // log::info!("{alias} random: {}", hex::encode(bytes));
        self.cx.push_randomness(bytes);
    }
}
