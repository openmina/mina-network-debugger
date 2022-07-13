use std::{collections::BTreeMap, net::SocketAddr};

use super::connection::{Connection, ConnectionId};

#[derive(Default)]
pub struct P2pRecorder {
    cns: BTreeMap<ConnectionId, Connection>,
}

impl P2pRecorder {
    pub fn on_connect(&mut self, incoming: bool, pid: u32, addr: SocketAddr) {
        let id = ConnectionId { pid, addr };
        if incoming {
            log::info!("accept {id:?}");
        } else {
            log::info!("connect {id:?}");
        }
        self.cns.insert(id, Connection::default());
    }

    pub fn on_disconnect(&mut self, pid: u32, addr: SocketAddr) {
        let id = ConnectionId { pid, addr };
        log::info!("disconnect {id:?}");
        self.cns.remove(&id);
    }

    pub fn on_data(&mut self, incoming: bool, pid: u32, addr: SocketAddr, bytes: Vec<u8>) {
        let id = ConnectionId { pid, addr };
        log::info!("{id:?} <- {}, {}", bytes.len(), hex::encode(&bytes));
        if let Some(cn) = self.cns.get_mut(&id) {
            cn.on_data(incoming, bytes);
        }
    }
}
