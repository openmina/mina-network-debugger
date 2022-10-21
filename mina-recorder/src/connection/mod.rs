use super::{
    recorder::Cx,
    event::DirectedId,
    database::{DbGroup as Db, DbResult, StreamId},
};

pub trait DynamicProtocol {
    fn from_name(name: &str, stream_id: StreamId) -> Self;
}

pub trait HandleData {
    // TODO: use Cow for bytes
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()>;
}

pub mod pnet;
pub mod multistream_select;
pub mod noise;
pub mod mux;
pub mod mplex;
pub mod yamux;
pub mod mina_protocol;
