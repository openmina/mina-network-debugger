use std::{
    collections::{BTreeMap, VecDeque},
    net::SocketAddr,
};

use super::connection::{ConnectionId, HandleData, pnet, multistream_select, chunk, noise, mplex};

type Cn = pnet::State<multistream_select::State<Noise>>;
type Noise = chunk::State<noise::State<Encrypted>>;
type Encrypted = multistream_select::State<mplex::State<()>>;

#[derive(Default)]
pub struct P2pRecorder {
    cns: BTreeMap<ConnectionId, Cn>,
    randomness: VecDeque<[u8; 32]>,
}

impl P2pRecorder {
    pub fn on_connect(&mut self, incoming: bool, alias: String, addr: SocketAddr, fd: u32) {
        if incoming {
            log::info!("{alias} accept {addr} {fd}");
        } else {
            log::info!("{alias} connect {addr} {fd}");
        }
        let id = ConnectionId { alias, addr, fd };
        self.cns.insert(id, Default::default());
    }

    pub fn on_disconnect(&mut self, alias: String, addr: SocketAddr, fd: u32) {
        log::info!("{alias} disconnect {addr} {fd}");
        let id = ConnectionId { alias, addr, fd };
        self.cns.remove(&id);
    }

    pub fn on_data(
        &mut self,
        incoming: bool,
        alias: String,
        addr: SocketAddr,
        fd: u32,
        mut bytes: Vec<u8>,
    ) {
        let id = ConnectionId { alias, addr, fd };
        if let Some(cn) = self.cns.get_mut(&id) {
            cn.on_data(id, incoming, &mut bytes, &mut self.randomness);
        }
    }

    pub fn on_randomness(&mut self, alias: String, bytes: [u8; 32]) {
        log::info!("{alias} random: {}", hex::encode(bytes));
        self.randomness.push_back(bytes);
    }
}
