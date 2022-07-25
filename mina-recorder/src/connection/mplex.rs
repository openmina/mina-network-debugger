use std::collections::VecDeque;

use unsigned_varint::decode;

use super::{DirectedId, HandleData};

#[derive(Default)]
pub struct State<Inner> {
    inner: Inner,
}

#[derive(Debug)]
enum Header {
    NewStream,
    MessageReceiver,
    MessageInitiator,
    CloseReceiver,
    CloseInitiator,
    ResetReceiver,
    ResetInitiator,
    Unknown,
}

impl Header {
    pub fn new(v: u64) -> Self {
        match v & 7 {
            0 => Header::NewStream,
            1 => Header::MessageReceiver,
            2 => Header::MessageInitiator,
            3 => Header::CloseReceiver,
            4 => Header::CloseInitiator,
            5 => Header::ResetReceiver,
            6 => Header::ResetInitiator,
            7 => Header::Unknown,
            _ => unreachable!(),
        }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData,
{
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], randomness: &mut VecDeque<[u8; 32]>) {
        let (v, remaining) = decode::u64(bytes).unwrap();
        let header = Header::new(v);
        let stream_id = v >> 3;

        let (len, remaining) = decode::usize(remaining).unwrap();
        let offset = remaining.as_ptr() as usize - bytes.as_ptr() as usize;

        log::info!("{id} {header:?} stream id: {stream_id}");

        // assert_eq!(offset + len, bytes.len());

        // TODO:
        if offset + len == bytes.len() {
            self.inner
                .on_data(id, &mut bytes[offset..(offset + len)], randomness)
        } else {
            self.inner
                .on_data(id.clone(), &mut bytes[offset..(offset + len)], randomness);
            self.on_data(id, &mut bytes[(offset + len)..], randomness);
        }
    }
}
