use prost::decode_length_delimiter;

use super::accumulator;

#[derive(Default)]
pub struct State(accumulator::State);

impl State {
    fn decode_size(mut bytes: &[u8]) -> Option<(usize, usize)> {
        let original = <&[u8]>::clone(&bytes);
        let l1 = decode_length_delimiter(&mut bytes).ok()?;
        let l0 = original.len() - bytes.len();
        Some((l0, l1))
    }

    /// Try accept immediately, without accumulation
    /// if returns false, the `bytes` contains full message, and accumulator is empty
    pub fn extend(&mut self, bytes: &[u8]) -> bool {
        self.0.extend(Self::decode_size, bytes)
    }

    pub fn next_msg(&mut self) -> Option<&[u8]> {
        self.0.next_msg(Self::decode_size)
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
        assert_ne!(st.0.pos(), 0);
        assert!(st.next_msg().is_some());
        assert!(st.next_msg().is_none());
        assert_eq!(st.0.pos(), 0);
    }
}
