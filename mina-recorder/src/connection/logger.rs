use std::collections::VecDeque;

use super::{ConnectionId, HandleData};

impl HandleData for () {
    fn on_data(
        &mut self,
        id: ConnectionId,
        incoming: bool,
        bytes: &mut [u8],
        _randomness: &mut VecDeque<[u8; 32]>,
    ) {
        let ConnectionId { alias, addr, fd } = id;
        let arrow = if incoming { "->" } else { "<-" };
        let len = bytes.len();
        log::info!(
            "{addr} {fd} {arrow} {alias} ({len} \"{}\")",
            hex::encode(bytes)
        );
    }
}

pub struct Raw;

impl HandleData for Raw {
    fn on_data(
        &mut self,
        id: ConnectionId,
        incoming: bool,
        bytes: &mut [u8],
        _randomness: &mut VecDeque<[u8; 32]>,
    ) {
        let ConnectionId { alias, addr, fd } = id;
        let arrow = if incoming { "->" } else { "<-" };
        let len = bytes.len();
        log::info!(
            "{addr} {fd} {arrow} {alias} raw({len} \"{}\")",
            hex::encode(bytes)
        );
    }
}
