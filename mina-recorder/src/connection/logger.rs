use std::collections::VecDeque;

use super::{DirectedId, HandleData};

impl HandleData for () {
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _randomness: &mut VecDeque<[u8; 32]>) {
        log::info!("{id} ({} \"{}\")", bytes.len(), hex::encode(bytes));
    }
}

pub struct Raw;

impl HandleData for Raw {
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _randomness: &mut VecDeque<[u8; 32]>) {
        log::info!("{id} raw({} \"{}\")", bytes.len(), hex::encode(bytes));
    }
}
