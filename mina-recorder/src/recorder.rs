use std::{collections::BTreeMap, net::SocketAddr};

use super::connection::{ConnectionId, HandleData, pnet, multistream_select, noise};

#[derive(Default)]
pub struct P2pRecorder {
    cns: BTreeMap<ConnectionId, pnet::State<multistream_select::State<noise::State<()>>>>,
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
        bytes: Vec<u8>,
    ) {
        let id = ConnectionId { alias, addr, fd };
        if let Some(cn) = self.cns.get_mut(&id) {
            cn.on_data(id, incoming, bytes);
        }
    }
}
