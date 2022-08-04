use std::fmt;

use super::{DirectedId, HandleData, DynamicProtocol, Cx};

impl DynamicProtocol for () {
    fn from_name(name: &str) -> Self {
        let _ = name;
    }
}

pub struct Output(Option<String>);

impl fmt::Display for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.as_ref().unwrap().fmt(f)
    }
}

impl Iterator for Output {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.take()
    }
}

impl HandleData for () {
    type Output = Output;

    #[inline(never)]
    fn on_data(&mut self, _id: DirectedId, bytes: &mut [u8], _cx: &mut Cx) -> Self::Output {
        Output(Some(format!(
            "({} \"{}\")",
            bytes.len(),
            hex::encode(bytes)
        )))
    }
}
