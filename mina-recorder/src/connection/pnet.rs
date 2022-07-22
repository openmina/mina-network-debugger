use std::collections::VecDeque;

use salsa20::cipher::generic_array::{typenum, GenericArray};
use salsa20::{
    cipher::{KeyIvInit as _, StreamCipher},
    XSalsa20,
};

use super::{ConnectionId, HandleData};

#[derive(Default)]
pub struct State<Inner> {
    cipher_in: Option<XSalsa20>,
    cipher_out: Option<XSalsa20>,
    inner: Inner,
}

impl<Inner> State<Inner> {
    pub fn shared_secret() -> GenericArray<u8, typenum::U32> {
        use blake2::{
            digest::{Update, VariableOutput},
            Blake2bVar,
        };

        // TODO: seed
        let seed = b"/coda/0.0.1/dd0f3f26be5a093f00077d1cd5d89abc253c95f301e9c12ae59e2d7c6052cc4d";
        let mut key = GenericArray::default();
        Blake2bVar::new(32)
            .unwrap()
            .chain(seed)
            .finalize_variable(&mut key)
            .unwrap();
        key
    }
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
        let cipher = if incoming {
            &mut self.cipher_in
        } else {
            &mut self.cipher_out
        };
        if let Some(cipher) = cipher {
            cipher.apply_keystream(bytes);
            self.inner.on_data(id, incoming, bytes, randomness);
        } else {
            assert_eq!(bytes.len(), 24);
            let key = Self::shared_secret();
            *cipher = Some(XSalsa20::new(&key, GenericArray::from_slice(bytes)));
        }
    }
}
