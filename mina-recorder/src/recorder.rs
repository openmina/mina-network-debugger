use std::{collections::BTreeMap, net::SocketAddr};

use super::connection::{Connection, ConnectionId};

#[derive(Default)]
pub struct P2pRecorder {
    cns: BTreeMap<ConnectionId, Connection>,
}

impl P2pRecorder {
    pub fn on_connect(&mut self, incoming: bool, alias: String, addr: SocketAddr) {
        if incoming {
            log::info!("{alias} accept {addr}");
        } else {
            log::info!("{alias} connect {addr}");
        }
        let id = ConnectionId { alias, addr };
        self.cns.insert(id, Connection::default());
    }

    pub fn on_disconnect(&mut self, alias: String, addr: SocketAddr) {
        log::info!("{alias} disconnect {addr}");
        let id = ConnectionId { alias, addr };
        self.cns.remove(&id);
    }

    pub fn on_data(&mut self, incoming: bool, alias: String, addr: SocketAddr, bytes: Vec<u8>) {
        if incoming {
            log::info!("{addr} -> {alias} {}, {}", bytes.len(), hex::encode(&bytes));
        } else {
            log::info!("{addr} <- {alias} {}, {}", bytes.len(), hex::encode(&bytes));
        }
        let id = ConnectionId { alias, addr };
        if let Some(cn) = self.cns.get_mut(&id) {
            cn.on_data(incoming, bytes);
        }
    }
}
