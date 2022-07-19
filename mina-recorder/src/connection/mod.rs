use std::{net::SocketAddr, collections::VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId {
    pub alias: String,
    pub addr: SocketAddr,
    pub fd: u32,
}

pub trait HandleData {
    fn on_data(
        &mut self,
        id: ConnectionId,
        incoming: bool,
        bytes: Vec<u8>,
        randomness: &mut VecDeque<[u8; 32]>,
    );
}

pub mod pnet;
pub mod multistream_select;
pub mod chunk;
pub mod noise;
