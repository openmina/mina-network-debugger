use super::{DirectedId, HandleData, Cx};

impl HandleData for () {
    type Output = String;

    fn on_data(&mut self, _id: DirectedId, bytes: &mut [u8], _cx: &mut Cx) -> Self::Output {
        format!("({} \"{}\")", bytes.len(), hex::encode(bytes))
    }
}
