use super::{DirectedId, HandleData, Cx};

#[derive(Default)]
pub struct State {}

impl HandleData for State {
    type Output = String;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        format!("here {}", ().on_data(id, bytes, cx))
    }
}
