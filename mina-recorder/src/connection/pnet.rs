use std::fmt;

use salsa20::cipher::generic_array::{typenum, GenericArray};
use salsa20::{
    cipher::{KeyIvInit as _, StreamCipher},
    XSalsa20,
};

use super::{DirectedId, HandleData, Cx};

#[derive(Default)]
pub struct State<Inner> {
    cipher_in: Option<XSalsa20>,
    cipher_out: Option<XSalsa20>,
    skip: bool,
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

pub enum Output<Inner> {
    Skip,
    HaveNonce,
    Inner(Inner),
}

impl<Inner> fmt::Display for Output<Inner>
where
    Inner: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Output::Skip => write!(f, "skip"),
            Output::HaveNonce => write!(f, "24 byte nonce"),
            Output::Inner(inner) => write!(f, "{inner}"),
        }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData,
{
    type Output = Output<Inner::Output>;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        if self.skip {
            return Output::Skip;
        }

        let cipher = if id.incoming {
            &mut self.cipher_in
        } else {
            &mut self.cipher_out
        };
        if let Some(cipher) = cipher {
            cipher.apply_keystream(bytes);
            let inner_out = self.inner.on_data(id, bytes, cx);
            Output::Inner(inner_out)
        } else if bytes.len() != 24 {
            self.skip = true;
            Output::Skip
        } else {
            let key = Self::shared_secret();
            *cipher = Some(XSalsa20::new(&key, GenericArray::from_slice(bytes)));
            Output::HaveNonce
        }
    }
}
