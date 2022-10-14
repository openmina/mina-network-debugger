use super::{
    recorder::Cx,
    event::DirectedId,
    database::{DbGroup as Db, DbResult},
};

pub trait DynamicProtocol {
    fn from_name(name: &str, id: u64, forward: bool) -> Self;
}

pub trait HandleData {
    // TODO: use Cow for bytes
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()>;
}

pub mod pnet;
pub mod multistream_select;
pub mod noise;
pub mod mplex;
pub mod mplex_;
pub mod mina_protocol;
