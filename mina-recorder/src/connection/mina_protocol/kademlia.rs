
use super::{DirectedId, HandleData, Cx};

#[allow(clippy::derive_partial_eq_without_eq)]
mod kad {
    include!(concat!(env!("OUT_DIR"), "/kad.pb.rs"));
}

#[derive(Default)]
pub struct State {}

impl HandleData for State {
    type Output = String;

    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        use prost::{bytes::Bytes, Message};

        let _ = (id, cx);

        let buf = Bytes::from(bytes.to_vec());
        let msg = <kad::Message as Message>::decode_length_delimited(buf).unwrap();

        format!("{:?}", msg.r#type())
    }
}
