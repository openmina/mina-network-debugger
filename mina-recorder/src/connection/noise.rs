use super::{ConnectionId, HandleData};

#[derive(Default)]
pub struct State<Inner> {
    inner: Inner,
}

impl<Inner> HandleData for State<Inner> {
    fn on_data(&mut self, id: ConnectionId, incoming: bool, bytes: Vec<u8>) {
        let ConnectionId { alias, addr, fd } = id;
        if incoming {
            log::info!(
                "{addr} {fd} -> {alias} {}, {}",
                bytes.len(),
                hex::encode(&bytes)
            );
        } else {
            log::info!(
                "{addr} {fd} <- {alias} {}, {}",
                bytes.len(),
                hex::encode(&bytes)
            );
        }
        let _ = &mut self.inner;
    }
}
