use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId {
    pub alias: String,
    pub addr: SocketAddr,
}

#[derive(Default)]
pub struct Connection {}

impl Connection {
    pub fn on_data(&mut self, incoming: bool, bytes: Vec<u8>) {
        let _ = (incoming, bytes);
    }
}
