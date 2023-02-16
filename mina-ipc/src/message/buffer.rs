use capnp::{
    message::Reader,
    serialize::{self, SliceSegments},
};

use super::{incoming, outgoing, Checksum};

#[derive(Default)]
pub struct Buffer<const INCOMING: bool> {
    bytes: Vec<u8>,
    checksum: Checksum,
}

impl<const INCOMING: bool> Buffer<INCOMING> {
    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.bytes.extend_from_slice(slice);
    }

    fn next_inner<T>(&mut self) -> Option<capnp::Result<T>>
    where
        T: for<'a> TryFrom<Reader<SliceSegments<'a>>, Error = capnp::Error>,
    {
        if !self.bytes.is_empty() {
            let mut slice = self.bytes.as_slice();

            match serialize::read_message_from_flat_slice(&mut slice, Default::default()) {
                Ok(value) => {
                    let consumed = self.bytes.len() - slice.len();
                    let r = match T::try_from(value) {
                        Ok(v) => v,
                        Err(err) => return Some(Err(err)),
                    };
                    self.checksum += &self.bytes[..consumed];
                    self.bytes = slice.to_vec();
                    Some(Ok(r))
                }
                Err(err) => match err.description.as_str() {
                    "failed to fill the whole buffer" | "empty slice" => None,
                    s if s.starts_with("Message ends prematurely.") => None,
                    _ => Some(Err(err)),
                },
            }
        } else {
            None
        }
    }

    pub fn checksum(&self) -> Checksum {
        self.checksum.clone()
    }
}

impl Iterator for Buffer<true> {
    type Item = capnp::Result<incoming::Msg>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_inner()
    }
}

impl Iterator for Buffer<false> {
    type Item = capnp::Result<outgoing::Msg>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_inner()
    }
}
