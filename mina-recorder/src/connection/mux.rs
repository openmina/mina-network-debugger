use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult, mplex, yamux, StreamId};

pub enum State<Inner> {
    Mplex(mplex::State<Inner>),
    Yamux(yamux::State<Inner>),
}

impl<Inner> DynamicProtocol for State<Inner> {
    fn from_name(name: &str, stream_id: StreamId) -> Self {
        match name {
            "/coda/mplex/1.0.0" => State::Mplex(mplex::State::from_name(name, stream_id)),
            "/coda/yamux/1.0.0" => State::Yamux(yamux::State::from_name(name, stream_id)),
            n => panic!("unexpected mux protocol: {n}"),
        }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + From<StreamId>,
{
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &Cx, db: &mut Db) -> DbResult<()> {
        match self {
            State::Mplex(state) => state.on_data(id, bytes, cx, db),
            State::Yamux(state) => state.on_data(id, bytes, cx, db),
        }
    }
}
