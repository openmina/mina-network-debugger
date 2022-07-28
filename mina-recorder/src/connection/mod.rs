use super::{recorder::Cx, DirectedId};

pub trait DynamicProtocol {
    fn from_name(name: &str) -> Self;
}

pub trait HandleData {
    type Output;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output;
}

pub mod pnet;
pub mod multistream_select;
pub mod noise;
pub mod mplex;
pub mod logger;
pub mod mina_protocol;
