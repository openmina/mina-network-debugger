use std::fmt;

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db};

impl DynamicProtocol for () {
    fn from_name(_: &str, _: u64, _: bool) -> Self {}
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
    fn on_data(&mut self, _: DirectedId, bytes: &mut [u8], _: &mut Cx, _: &Db) -> Self::Output {
        Output(Some(format!(
            "({} \"{}\")",
            bytes.len(),
            hex::encode(bytes)
        )))
    }
}
