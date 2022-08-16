use salsa20::cipher::generic_array::{typenum, GenericArray};
use salsa20::{
    cipher::{KeyIvInit as _, StreamCipher},
    XSalsa20,
};

use crate::database::DbStream;

use super::{HandleData, DirectedId, Cx, Db};

pub struct State<Inner> {
    cipher_in: Option<XSalsa20>,
    cipher_out: Option<XSalsa20>,
    // TODO: raw stream in separate cf
    _stream: Option<DbStream>,
    skip: bool,
    inner: Inner,
}

impl<Inner> Default for State<Inner>
where
    Inner: From<(u64, bool)>,
{
    fn default() -> Self {
        State {
            cipher_in: None,
            cipher_out: None,
            _stream: None,
            skip: false,
            inner: Inner::from((0, false)),
        }
    }
}

impl<Inner> State<Inner> {
    pub fn shared_secret() -> GenericArray<u8, typenum::U32> {
        use blake2::{
            digest::{Update, VariableOutput},
            Blake2bVar,
        };

        // TODO: seed
        // local sandbox dd0f3f26be5a093f00077d1cd5d89abc253c95f301e9c12ae59e2d7c6052cc4d
        // mainnet 5f704cc0c82e0ed70e873f0893d7e06f148524e3f0bdae2afb02e7819a0c24d1
        let seed = b"/coda/0.0.1/5f704cc0c82e0ed70e873f0893d7e06f148524e3f0bdae2afb02e7819a0c24d1";
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
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) {
        let cipher = if id.incoming {
            &mut self.cipher_in
        } else {
            &mut self.cipher_out
        };
        if let Some(cipher) = cipher {
            // TODO: raw
            // self.stream.as_ref().unwrap().add(id.incoming, id.metadata.time, bytes).unwrap();
            cipher.apply_keystream(bytes);
            self.inner.on_data(id, bytes, cx, db);
        } else if bytes.len() != 24 {
            self.skip = true;
            log::warn!("skip connection {id}, bytes: {}", hex::encode(bytes));
        } else {
            let key = Self::shared_secret();
            *cipher = Some(XSalsa20::new(&key, GenericArray::from_slice(bytes)));
            // self.stream = Some(db.add(StreamMeta::Raw, StreamKind::Raw).unwrap());
        }
    }
}
