use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId {
    pub alias: String,
    pub addr: SocketAddr,
    pub fd: u32,
}

pub trait HandleData {
    fn on_data(&mut self, id: ConnectionId, incoming: bool, bytes: Vec<u8>);
}

pub mod pnet;
pub mod multistream_select;
pub mod noise;
