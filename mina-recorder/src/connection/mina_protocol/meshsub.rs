use prost::decode_length_delimiter;

#[derive(Default)]
pub struct State {
    pos: usize,
    acc: Vec<u8>,
}

impl State {
    /// Try accept immediately, without accumulation
    /// if returns false, the `bytes` contains full message, and accumulator is empty
    pub fn extend(&mut self, mut bytes: &[u8]) -> bool {
        let original = <&[u8]>::clone(&bytes);
        if self.acc.is_empty() {
            if decode_length_delimiter(&mut bytes).unwrap() as usize == bytes.len() {
                return false;
            }
        }
        self.acc.extend_from_slice(original);
        true
    }

    fn drop_buffer(&mut self) {
        self.acc = self.acc[self.pos..].to_vec();
        self.pos = 0;
    }

    pub fn next_msg(&mut self) -> Option<&[u8]> {
        if self.acc.is_empty() || self.acc.len() == self.pos {
            self.drop_buffer();
            return None;
        }

        let mut bytes = &self.acc.as_slice()[self.pos..];
        let len = decode_length_delimiter(&mut bytes).ok()? as usize;
        if bytes.len() >= len {
            let new_pos = self.acc.len() - bytes.len() + len;
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

#[cfg(test)]
mod tests {
    #[test]
    fn meshsub_short() {
        let msg = "230a210801121d636f64612f636f6e73656e7375732d6d657373616765732f302e302e31231a211a1f0a1d636f64612f636f6e73656e7375732d6d657373616765732f302e302e31";
        let msg = hex::decode(msg).unwrap();
        let mut st = super::State::default();
        assert!(st.extend(&msg));
        assert!(st.next_msg().is_some());
        assert_ne!(st.pos, 0);
        assert!(st.next_msg().is_some());
        assert!(st.next_msg().is_none());
        assert_eq!(st.pos, 0);
    }
}
