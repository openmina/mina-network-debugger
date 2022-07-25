use super::{DirectedId, HandleData, Cx};

impl HandleData for () {
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _cx: &mut Cx) {
        log::info!("{id} ({} \"{}\")", bytes.len(), hex::encode(bytes));
    }
}

pub struct Raw;

impl HandleData for Raw {
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _cx: &mut Cx) {
        log::info!("{id} raw({} \"{}\")", bytes.len(), hex::encode(bytes));
    }
}
