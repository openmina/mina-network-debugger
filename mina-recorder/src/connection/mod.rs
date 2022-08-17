use super::{
    recorder::Cx,
    DirectedId,
    database::{DbGroup as Db, DbResult},
};

pub trait DynamicProtocol {
    fn from_name(name: &str, id: u64, forward: bool) -> Self;
}

pub trait HandleData {
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()>;
}

pub mod pnet;
pub mod multistream_select;
pub mod noise;
pub mod mplex;
pub mod mina_protocol;
