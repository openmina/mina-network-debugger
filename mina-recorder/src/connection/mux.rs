use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult, mplex, yamux};

pub enum State<Inner> {
    Mplex(mplex::State<Inner>),
    Yamux(yamux::State<Inner>),
}

impl<Inner> DynamicProtocol for State<Inner> {
    fn from_name(name: &str, id: u64, forward: bool) -> Self {
        match name {
            "/coda/mplex/1.0.0" => State::Mplex(mplex::State::from_name(name, id, forward)),
            "/coda/yamux/1.0.0" => State::Yamux(yamux::State::from_name(name, id, forward)),
            n => panic!("unexpected mux protocol: {n}"),
        }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + From<(u64, bool)>,
{
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        match self {
            State::Mplex(state) => state.on_data(id, bytes, cx, db),
            State::Yamux(state) => state.on_data(id, bytes, cx, db),
        }
    }
}
