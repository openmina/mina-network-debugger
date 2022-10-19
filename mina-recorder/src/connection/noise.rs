use std::ops::Range;

use curve25519_dalek::{
    scalar::Scalar, constants::ED25519_BASEPOINT_TABLE, montgomery::MontgomeryPoint,
};
use sha2::Sha256;
use chacha20poly1305::ChaCha20Poly1305;
use vru_noise::{
    SymmetricState,
    hkdf::hmac::Hmac,
    generic_array::{typenum, GenericArray},
    ChainingKey, Key, Cipher, Output,
};
use thiserror::Error;

use crate::{
    database::{DbStream, StreamId, StreamKind, RandomnessDatabase},
    chunk::EncryptionStatus,
};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

type C = (Hmac<Sha256>, Sha256, typenum::B0, ChaCha20Poly1305);

pub type State<Inner> = ChunkState<NoiseState<Inner>>;

#[derive(Default)]
pub struct ChunkState<Inner> {
    accumulator_incoming: Vec<u8>,
    accumulator_outgoing: Vec<u8>,
    inner: Inner,
}

impl<Inner> DynamicProtocol for ChunkState<Inner>
where
    Inner: DynamicProtocol,
{
    fn from_name(name: &str, id: u64, forward: bool) -> Self {
        ChunkState {
            accumulator_incoming: vec![],
            accumulator_outgoing: vec![],
            inner: Inner::from_name(name, id, forward),
        }
    }
}

impl<Inner> HandleData for ChunkState<Inner>
where
    Inner: HandleData,
{
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        let accumulator = if id.incoming {
            &mut self.accumulator_incoming
        } else {
            &mut self.accumulator_outgoing
        };

        if accumulator.is_empty() && bytes.len() >= 2 {
            let len = u16::from_be_bytes(bytes[..2].try_into().expect("checked above")) as usize;
            if bytes.len() == 2 + len {
                return self.inner.on_data(id, bytes, cx, db);
            }
        }

        accumulator.extend_from_slice(bytes);
        loop {
            if accumulator.len() >= 2 {
                let len = accumulator[..2].try_into().expect("checked above");
                let len = u16::from_be_bytes(len) as usize;
                if accumulator.len() >= 2 + len {
                    let (chunk, remaining) = accumulator.split_at_mut(2 + len);
                    self.inner.on_data(id.clone(), chunk, cx, db)?;
                    *accumulator = remaining.to_vec();
                    continue;
                }
            }
            break;
        }

        Ok(())
    }
}

pub struct NoiseState<Inner> {
    machine: Option<St>,
    stream: Option<DbStream>,
    initiator_is_incoming: bool,
    error: bool,
    inner: Inner,
    decrypted: usize,
    failed_to_decrypt: usize,
}

impl<Inner> DynamicProtocol for NoiseState<Inner>
where
    Inner: From<(u64, bool)>,
{
    fn from_name(name: &str, _: u64, _: bool) -> Self {
        if name != "/noise" {
            NoiseState {
                error: true,
                ..Default::default()
            }
        } else {
            Self::default()
        }
    }
}

impl<Inner> Default for NoiseState<Inner>
where
    Inner: From<(u64, bool)>,
{
    fn default() -> Self {
        NoiseState {
            machine: None,
            stream: None,
            initiator_is_incoming: false,
            error: false,
            inner: Inner::from((0, false)),
            decrypted: 0,
            failed_to_decrypt: 0,
        }
    }
}

enum St {
    FirstMessage {
        st: SymmetricState<C, ChainingKey<C>>,
        i_epk: MontgomeryPoint,
    },
    SecondMessage {
        st: SymmetricState<C, Key<C, typenum::U1>>,
        r_epk: MontgomeryPoint,
    },
    Transport {
        initiators_receiver: Cipher<C, 1, false>,
        responders_receiver: Cipher<C, 1, false>,
    },
}

impl<Inner> HandleData for NoiseState<Inner>
where
    Inner: HandleData,
{
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        enum Msg {
            First,
            Second,
            Third,
            Other,
        }

        let msg = match &self.machine {
            None => Msg::First,
            Some(St::FirstMessage { .. }) => Msg::Second,
            Some(St::SecondMessage { .. }) => Msg::Third,
            Some(_) => Msg::Other,
        };
        if !self.error {
            match self.on_data_(id.incoming, bytes, &cx.db.core()) {
                Ok(range) => {
                    let bytes = &mut bytes[range];
                    match msg {
                        Msg::First => (),
                        Msg::Second => {
                            self.decrypted += bytes.len();
                            cx.stats.decrypted += bytes.len();
                            let stream = self.stream.get_or_insert_with(|| {
                                db.add(StreamId::Handshake, StreamKind::Handshake)
                            });
                            stream.add(&id, bytes)?;
                        }
                        Msg::Third => {
                            self.decrypted += bytes.len();
                            cx.stats.decrypted += bytes.len();
                            self.stream
                                .as_ref()
                                .expect("must have stream at third message")
                                .add(&id, bytes)?;
                        }
                        Msg::Other => {
                            self.decrypted += bytes.len();
                            cx.stats.decrypted += bytes.len();
                            db.add_raw(
                                EncryptionStatus::DecryptedNoise,
                                id.incoming,
                                id.metadata.time,
                                bytes,
                            )?;
                            self.inner.on_data(id, bytes, cx, db)?;
                        }
                    }
                }
                Err(err) => {
                    self.error = true;
                    self.on_error(id, bytes, cx, db, err)?;
                }
            }
        } else {
            self.on_error(id, bytes, cx, db, NoiseError::CannotDecrypt)?;
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum NoiseError {
    #[error("first message too short")]
    FirstMessageTooShort,
    #[error("first message too big")]
    FirstMessageTooBig,
    #[error("second message too short")]
    SecondMessageTooShort,
    #[error("third message too short")]
    ThirdMessageTooShort,
    #[error("second message mac mismatch")]
    SecondMessageMacMismatch,
    #[error("second message payload mac mismatch")]
    SecondMessagePayloadMacMismatch,
    #[error("third message mac mismatch")]
    ThirdMessageMacMismatch,
    #[error("third message payload mac mismatch")]
    ThirdMessagePayloadMacMismatch,
    #[error("ephemeral secret key not found {i}, r_epk: {}, i_epk: {}", hex::encode(r_epk.as_bytes()), hex::encode(i_epk.as_bytes()))]
    EphemeralSecretKeyNotFound {
        i: bool,
        r_epk: MontgomeryPoint,
        i_epk: MontgomeryPoint,
    },
    #[error("secret key not found {i}, r_spk: {}, i_epk: {}", hex::encode(r_spk.as_bytes()), hex::encode(i_epk.as_bytes()))]
    SecondSecretKeyNotFound {
        i: bool,
        r_spk: MontgomeryPoint,
        i_epk: MontgomeryPoint,
    },
    #[error("ephemeral secret key not found {i}, r_epk: {}, i_spk: {}", hex::encode(r_epk.as_bytes()), hex::encode(i_spk.as_bytes()))]
    ThirdSecretKeyNotFound {
        i: bool,
        r_epk: MontgomeryPoint,
        i_spk: MontgomeryPoint,
    },
    #[error("data too short")]
    DataTooShort,
    #[error("cannot decrypt")]
    CannotDecrypt,
}

impl<Inner> NoiseState<Inner> {
    fn on_error(
        &mut self,
        id: DirectedId,
        bytes: &mut [u8],
        cx: &mut Cx,
        db: &Db,
        err: NoiseError,
    ) -> DbResult<()> {
        cx.stats.failed_to_decrypt += bytes.len();
        self.failed_to_decrypt += bytes.len();

        log::error!(
            "{id} {}, total failed {}, total decrypted {}, {err}: {} {}...",
            db.id(),
            cx.stats.failed_to_decrypt,
            cx.stats.decrypted,
            bytes.len(),
            hex::encode(&bytes[..32.min(bytes.len())])
        );

        let stream = self
            .stream
            .get_or_insert_with(|| db.add(StreamId::Handshake, StreamKind::Handshake));
        let mut b = b"mac_mismatch\x00\x00\x00\x00".to_vec();
        b.extend_from_slice(&self.decrypted.to_be_bytes());
        b.extend_from_slice(&self.failed_to_decrypt.to_be_bytes());
        b.extend_from_slice(&cx.stats.decrypted.to_be_bytes());
        b.extend_from_slice(&cx.stats.failed_to_decrypt.to_be_bytes());
        stream.add(&id, &b)?;

        Ok(())
    }

    fn on_data_<'a>(
        &mut self,
        incoming: bool,
        bytes: &'a mut [u8],
        cx: &impl RandomnessDatabase,
    ) -> Result<Range<usize>, NoiseError> {
        fn find_sk(pk: &MontgomeryPoint, cx: &impl RandomnessDatabase) -> Option<Scalar> {
            let sk = cx.iterate_randomness().find_map(|rand| {
                if rand.len() != 32 {
                    return None;
                }
                let mut sk_bytes = [0; 32];
                sk_bytes.clone_from_slice(&rand);
                sk_bytes[0] &= 248;
                sk_bytes[31] &= 127;
                sk_bytes[31] |= 64;
                let sk = Scalar::from_bits(sk_bytes);
                if (&ED25519_BASEPOINT_TABLE * &sk).to_montgomery().eq(pk) {
                    Some(sk)
                } else {
                    None
                }
            })?;
            Some(sk)
        }

        fn try_dh(
            a: &MontgomeryPoint,
            b: &MontgomeryPoint,
            cx: &impl RandomnessDatabase,
        ) -> Option<[u8; 32]> {
            find_sk(a, cx)
                .map(|sk| b * sk)
                .or_else(|| find_sk(b, cx).map(|sk| a * sk))
                .map(|ss| ss.to_bytes())
        }

        let range;
        let len = bytes.len();
        self.machine = match self.machine.take() {
            None => {
                self.initiator_is_incoming = incoming;

                #[allow(clippy::comparison_chain)]
                if bytes.len() < 34 {
                    return Err(NoiseError::FirstMessageTooShort);
                } else if bytes.len() > 34 {
                    return Err(NoiseError::FirstMessageTooBig);
                }

                let i_epk =
                    MontgomeryPoint(bytes[2..34].try_into().expect("cannot fail, checked above"));
                let st = SymmetricState::new("Noise_XX_25519_ChaChaPoly_SHA256")
                    .mix_hash(&[])
                    .mix_hash(i_epk.as_bytes())
                    .mix_hash(&[]);
                range = 34..len;
                Some(St::FirstMessage { st, i_epk })
            }
            Some(St::FirstMessage { st, i_epk }) => {
                if bytes.len() < 98 {
                    return Err(NoiseError::SecondMessageTooShort);
                }

                let r_epk =
                    MontgomeryPoint(bytes[2..34].try_into().expect("cannot fail, checked above"));

                let mut r_spk_bytes: [u8; 32] = bytes[34..66]
                    .try_into()
                    .expect("cannot fail, checked above");
                let tag = *GenericArray::from_slice(&bytes[66..82]);
                let r_spk;
                let payload_tag = *GenericArray::from_slice(&bytes[(len - 16)..]);
                let st = st
                    .mix_hash(r_epk.as_bytes())
                    .mix_shared_secret(try_dh(&r_epk, &i_epk, cx).ok_or_else(|| {
                        let i = self.initiator_is_incoming;
                        NoiseError::EphemeralSecretKeyNotFound { i, r_epk, i_epk }
                    })?)
                    .decrypt(&mut r_spk_bytes, &tag)
                    .map_err(|_| NoiseError::SecondMessageMacMismatch)?
                    .mix_shared_secret({
                        r_spk = MontgomeryPoint(r_spk_bytes);
                        try_dh(&r_spk, &i_epk, cx).ok_or_else(|| {
                            let i = self.initiator_is_incoming;
                            NoiseError::SecondSecretKeyNotFound { i, r_spk, i_epk }
                        })?
                    })
                    .decrypt(&mut bytes[82..(len - 16)], &payload_tag)
                    .map_err(|_| NoiseError::SecondMessagePayloadMacMismatch)?;

                range = 82..(len - 16);
                Some(St::SecondMessage { st, r_epk })
            }
            Some(St::SecondMessage { st, r_epk }) => {
                if bytes.len() < 98 {
                    return Err(NoiseError::ThirdMessageTooShort);
                }

                let mut i_spk_bytes: [u8; 32] =
                    bytes[2..34].try_into().expect("cannot fail, checked above");
                let tag = *GenericArray::from_slice(&bytes[34..50]);
                let i_spk;
                let payload_tag = *GenericArray::from_slice(&bytes[(len - 16)..]);
                let Output {
                    sender, receiver, ..
                } = st
                    .decrypt(&mut i_spk_bytes, &tag)
                    .map_err(|_| NoiseError::ThirdMessageMacMismatch)?
                    .mix_shared_secret({
                        i_spk = MontgomeryPoint(i_spk_bytes);
                        try_dh(&i_spk, &r_epk, cx).ok_or_else(|| {
                            let i = self.initiator_is_incoming;
                            NoiseError::ThirdSecretKeyNotFound { i, r_epk, i_spk }
                        })?
                    })
                    .decrypt(&mut bytes[50..(len - 16)], &payload_tag)
                    .map_err(|_| NoiseError::ThirdMessagePayloadMacMismatch)?
                    .finish::<1, true>();

                range = 50..(len - 16);
                Some(St::Transport {
                    initiators_receiver: receiver,
                    responders_receiver: sender.swap(),
                })
            }
            Some(St::Transport {
                mut initiators_receiver,
                mut responders_receiver,
            }) => {
                if len < 18 {
                    return Err(NoiseError::DataTooShort);
                }

                let payload_tag = *GenericArray::from_slice(&bytes[(len - 16)..]);
                let receiver = if incoming == self.initiator_is_incoming {
                    &mut initiators_receiver
                } else {
                    &mut responders_receiver
                };
                if receiver
                    .decrypt(&[], &mut bytes[2..(len - 16)], &payload_tag)
                    .is_err()
                {
                    return Err(NoiseError::CannotDecrypt);
                } else {
                    range = 2..(len - 16);
                }
                Some(St::Transport {
                    initiators_receiver,
                    responders_receiver,
                })
            }
        };
        Ok(range)
    }
}

#[cfg(test)]
#[test]
#[rustfmt::skip]
fn noise() {
    struct Randomness([Vec<u8>; 2]);

    impl RandomnessDatabase for Randomness {
        fn iterate_randomness<'a>(&'a self) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a> {
            Box::new(self.0.iter().map(|x| x.clone().into_boxed_slice()))
        }
    }

    let mut noise = NoiseState::<super::multistream_select::State<()>>::default();
    let mut cx = Randomness([
        hex::decode("d1f3bca173136dd555dd97262336ce644a76ec31d521d2befe87caec8678c1a7").expect("valid constant").try_into().expect("valid constant"),
        hex::decode("1c283e25c80f64f2806d9e19da1a393873d40bdf3d903a3776e013c4fdd97cb3").expect("valid constant").try_into().expect("valid constant"),
    ]);

    let mut id = DirectedId::default();
    id.incoming = true;
    noise.on_data_(id.incoming, &mut hex::decode("00209844288f8c8f0337dff411d66e0378d950fb7590f9f44d6df969fd59a18ab849").expect("valid constant"), &mut cx).expect("test");
    id.incoming = false;
    noise.on_data_(id.incoming, &mut hex::decode("00c8c0e8867216784ce23e6ad97120c8bfa139941424d0aebcdfe14e339798af4a377f2a97c280a913fdf6a96b4b89c5471a7f4761bec49a557d734b65495eb87e1e00b707d561da835698fe08bab7962b0491751110e8a32a260605a64dbdc18f503958be161fe9546f3c0494c0714f6e57c3eca413cec2d20a483855b4958b96ee79e05f34fa63a74c758ebe9537f4e1c733a7a7ebcd9b1bcc47c2c882ffa361f6ebb404225b60a6bae8e7a6d479d6e1b5c5c1d858ca13dde8cbd285f5bb4d9805578553e3881d5a0d").expect("valid constant"), &mut cx).expect("test");
    id.incoming = true;
    noise.on_data_(id.incoming, &mut hex::decode("00a8e3cfaddd47cf48db1b70b83c15dbdb32bdba21cca65f9f80fb2e7f93d7a82b1b71d6241952e1205d510afad46f8d6d23de1be013618cd79d4e87eec4761292393532e7952bddaeb6709dcb266f861f92ef0eabe282d318f813d11426ac6916240bfead8994c63f10b03f6e241c2b92495a1f63d728fb63ba78e468945f7da081761102465308523dbf50064be4251468abb99db7af8afd71b99100a2fb7a37773a8062d33cc2e1d9").expect("valid constant"), &mut cx).expect("test");
    id.incoming = true;
    noise.on_data_(id.incoming, &mut hex::decode("00375cd2640426acf52810f89147cf5446f8b4bff334c9727c0a45abd220746b2e8b10d269ff28be87c8bb1d53e43e69922ff4b19760ef875d").expect("valid constant"), &mut cx).expect("test");
}

#[cfg(test)]
#[test]
#[should_panic]
fn key_() {
    use std::convert::TryFrom;

    let esk_str = "b6b066bee2e2c0bebfc589bc8249ea6c10d14cd96c627b346eb5a84b4a59c540";
    let r_epk_str = "5c67bb93b3b14cea01918c0abcf443199301ee543d0ece74d5a9e5d7e4aae013";
    let i_epk_str = "32cb49257fd546029f3b8dbd00ed58384caa86a4c0b5c6a56debe209c77bd143";

    let mut esk = <[u8; 32]>::try_from(hex::decode(esk_str).unwrap().as_slice()).unwrap();
    let r_epk = <[u8; 32]>::try_from(hex::decode(r_epk_str).unwrap().as_slice()).unwrap();
    let i_epk = <[u8; 32]>::try_from(hex::decode(i_epk_str).unwrap().as_slice()).unwrap();

    esk[0] &= 248;
    esk[31] &= 127;
    esk[31] |= 64;
    let sk = Scalar::from_bits(esk);
    let pk = (&ED25519_BASEPOINT_TABLE * &sk).to_montgomery();
    println!("{}", hex::encode(pk.0));

    let r = MontgomeryPoint(r_epk).eq(&pk);
    let i = MontgomeryPoint(i_epk).eq(&pk);
    assert!(r | i);
}
