#[derive(Default)]
pub struct State {
    pos: usize,
    acc: Vec<u8>,
}

impl State {
    #[cfg(test)]
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Try accept immediately, without accumulation
    /// if returns false, the `bytes` contains full message, and accumulator is empty
    pub fn extend<F>(&mut self, decode_size: F, bytes: &[u8]) -> bool
    where
        // try decode size of message from bytes,
        // if bytes contains not enough information return `None`
        // otherwise return separately size of size information and size of msg
        F: Fn(&[u8]) -> Option<(usize, usize)>,
    {
        let original = <&[u8]>::clone(&bytes);
        if self.acc.is_empty() {
            if let Some((l0, l1)) = decode_size(bytes) {
                if l0 + l1 == bytes.len() {
                    return false;
                }
            }
        }
        self.acc.extend_from_slice(original);
        true
    }

    fn drop_buffer(&mut self) {
        self.acc = self.acc[self.pos..].to_vec();
        self.pos = 0;
    }

    pub fn next_msg<F>(&mut self, decode_size: F) -> Option<&[u8]>
    where
        F: Fn(&[u8]) -> Option<(usize, usize)>,
    {
        if self.acc.is_empty() || self.acc.len() == self.pos {
            self.drop_buffer();
            return None;
        }

        let bytes = &self.acc.as_slice()[self.pos..];
        let (l0, l1) = decode_size(bytes)?;
        if bytes.len() >= l0 + l1 {
            let new_pos = self.acc.len() - bytes.len() + l0 + l1;
            if self.pos == new_pos {
                self.drop_buffer();
                None
            } else {
                let s = &self.acc[self.pos..new_pos];
                self.pos = new_pos;
                Some(s)
            }
        } else {
            self.drop_buffer();
            None
        }
    }
}
