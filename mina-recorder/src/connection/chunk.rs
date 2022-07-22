use std::collections::VecDeque;

use super::{ConnectionId, HandleData};

#[derive(Default)]
pub struct State<Inner> {
    accumulator: Vec<u8>,
    inner: Inner,
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData,
{
    fn on_data(
        &mut self,
        id: ConnectionId,
        incoming: bool,
        bytes: &mut [u8],
        randomness: &mut VecDeque<[u8; 32]>,
    ) {
        if self.accumulator.is_empty() && bytes.len() >= 2 {
            let len = u16::from_be_bytes(bytes[..2].try_into().unwrap()) as usize;
            if bytes.len() == 2 + len {
                return self.inner.on_data(id, incoming, bytes, randomness);
            }
        }

        self.accumulator.extend_from_slice(bytes);
        loop {
            if self.accumulator.len() >= 2 {
                let len = u16::from_be_bytes(self.accumulator[..2].try_into().unwrap()) as usize;
                if self.accumulator.len() >= 2 + len {
                    let (chunk, remaining) = self.accumulator.split_at_mut(2 + len);
                    self.inner.on_data(id.clone(), incoming, chunk, randomness);
                    self.accumulator = remaining.to_vec();
                    continue;
                }
            }
            break;
        }
    }
}
