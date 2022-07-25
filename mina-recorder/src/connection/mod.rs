use std::{net::SocketAddr, collections::VecDeque, fmt};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId {
    pub alias: String,
    pub addr: SocketAddr,
    pub fd: u32,
}

#[derive(Clone)]
pub struct DirectedId {
    pub id: ConnectionId,
    pub incoming: bool,
}

impl fmt::Display for DirectedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ConnectionId { alias, addr, fd } = &self.id;
        let arrow = if self.incoming { "->" } else { "<-" };
        write!(f, "{addr} {fd} {arrow} {alias}")
    }
}

pub trait HandleData {
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], randomness: &mut VecDeque<[u8; 32]>);
}

pub mod pnet;
pub mod multistream_select;
pub mod chunk;
pub mod noise;
pub mod mplex;
pub mod logger;
