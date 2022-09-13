use salsa20::cipher::generic_array::{typenum, GenericArray};
use salsa20::{
    cipher::{KeyIvInit as _, StreamCipher},
    XSalsa20,
};

use super::{HandleData, DirectedId, Cx, Db, DbResult};

pub struct State<Inner> {
    shared_secret: GenericArray<u8, typenum::U32>,
    cipher_in: Option<XSalsa20>,
    cipher_out: Option<XSalsa20>,
    skip: bool,
    inner: Inner,
}

impl<Inner> State<Inner>
where
    Inner: From<(u64, bool)>,
{
    pub fn new(chain_id: &[u8]) -> Self {
        State {
            shared_secret: Self::shared_secret(chain_id),
            cipher_in: None,
            cipher_out: None,
            skip: false,
            inner: Inner::from((0, false)),
        }
    }
}

impl<Inner> State<Inner> {
    pub fn shared_secret(chain_id: &[u8]) -> GenericArray<u8, typenum::U32> {
        use blake2::{
            digest::{Update, VariableOutput},
            Blake2bVar,
        };

        let mut key = GenericArray::default();
        Blake2bVar::new(32)
            .expect("valid constant")
            .chain(chain_id)
            .finalize_variable(&mut key)
            .expect("good buffer size");
        key
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData,
{
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        let cipher = if id.incoming {
            &mut self.cipher_in
        } else {
            &mut self.cipher_out
        };
        if let Some(cipher) = cipher {
            cipher.apply_keystream(bytes);
            db.add_raw(true, id.incoming, id.metadata.time, bytes)?;
            self.inner.on_data(id, bytes, cx, db)?;
        } else if bytes.len() != 24 {
            self.skip = true;
            log::warn!("skip connection {id}, bytes: {}", hex::encode(bytes));
        } else {
            *cipher = Some(XSalsa20::new(
                &self.shared_secret,
                GenericArray::from_slice(bytes),
            ));
            // self.stream = Some(db.add(StreamMeta::Raw, StreamKind::Raw));
        }

        Ok(())
    }
}
