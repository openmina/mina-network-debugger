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

use crate::database::{DbStream, StreamId, StreamKind, ConnectionId};

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
    Inner: Default,
{
    fn from_name(name: &str, _: u64, _: bool) -> Self {
        assert_eq!(name, "/noise");
        Self::default()
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
            match self.on_data_(id.clone(), db.id(), bytes, cx) {
                Some(bytes) => match msg {
                    Msg::First => (),
                    Msg::Second => {
                        let stream = self.stream.get_or_insert_with(|| {
                            db.add(StreamId::Handshake, StreamKind::Handshake)
                        });
                        stream.add(id.incoming, id.metadata.time, bytes)?;
                    }
                    Msg::Third => {
                        self.stream
                            .as_ref()
                            .expect("must have stream at third message")
                            .add(id.incoming, id.metadata.time, bytes)?;
                    }
                    Msg::Other => {
                        cx.decrypted += bytes.len();
                        db.add_raw(false, id.incoming, id.metadata.time, bytes)?;
                        self.inner.on_data(id, bytes, cx, db)?;
                    }
                },
                None => {
                    cx.failed_to_decrypt += bytes.len();
                    log::error!(
                        "{id} {}, total failed {}, total decrypted {}, cannot decrypt: {} {}...",
                        db.id(),
                        cx.failed_to_decrypt,
                        cx.decrypted,
                        bytes.len(),
                        hex::encode(&bytes[..32.min(bytes.len())])
                    );
                }
            }
        } else {
            cx.failed_to_decrypt += bytes.len();
            log::error!(
                "{id} {}, total failed {}, total decrypted {}, cannot decrypt: {} {}...",
                db.id(),
                cx.failed_to_decrypt,
                cx.decrypted,
                bytes.len(),
                hex::encode(&bytes[..32.min(bytes.len())])
            );
        }

        Ok(())
    }
}

impl<Inner> NoiseState<Inner> {
    fn on_data_<'a>(
        &mut self,
        id: DirectedId,
        db_id: ConnectionId,
        bytes: &'a mut [u8],
        cx: &mut Cx,
    ) -> Option<&'a mut [u8]> {
        fn find_sk(pk: &MontgomeryPoint, cx: &Cx) -> Option<Scalar> {
            let sk = cx.db.core().iterate_randomness().find_map(|rand| {
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

        fn try_dh(a: &MontgomeryPoint, b: &MontgomeryPoint, cx: &Cx) -> Option<[u8; 32]> {
            find_sk(a, cx)
                .map(|sk| b * sk)
                .or_else(|| find_sk(b, cx).map(|sk| a * sk))
                .map(|ss| ss.to_bytes())
        }

        let range;
        let len = bytes.len();
        self.machine = match self.machine.take() {
            None => {
                self.initiator_is_incoming = id.incoming;

                #[allow(clippy::comparison_chain)]
                if bytes.len() < 34 {
                    log::error!(
                        "{id} {db_id}, first message too short {}",
                        hex::encode(bytes)
                    );
                    self.error = true;
                    return None;
                } else if bytes.len() > 34 {
                    log::error!(
                        "{id} {db_id}, suspicious first message {} {}",
                        bytes.len(),
                        hex::encode(&bytes)
                    );
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
                    log::error!(
                        "{id} {db_id}, second message too short {}",
                        hex::encode(bytes)
                    );
                    self.error = true;
                    return None;
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
                    .mix_shared_secret(try_dh(&r_epk, &i_epk, cx).or_else(|| {
                        log::error!(
                            "{id} {db_id}, ephemeral secret key not found, r_epk: {}, i_epk: {}",
                            hex::encode(r_epk.as_bytes()),
                            hex::encode(i_epk.as_bytes()),
                        );
                        self.error = true;
                        None
                    })?)
                    .decrypt(&mut r_spk_bytes, &tag)
                    .map_err(|err| {
                        log::error!("{id} {db_id}, second message mac mismatch");
                        self.error = true;
                        err
                    })
                    .ok()?
                    .mix_shared_secret({
                        r_spk = MontgomeryPoint(r_spk_bytes);
                        try_dh(&r_spk, &i_epk, cx).or_else(|| {
                            let i = if self.initiator_is_incoming {
                                "initiator_is_incoming"
                            } else {
                                "initiator_is_outgoing"
                            };
                            log::error!(
                                "{id} {db_id}, {i} secret key not found, r_spk: {}, i_epk: {}",
                                hex::encode(r_spk.as_bytes()),
                                hex::encode(i_epk.as_bytes()),
                            );
                            self.error = true;
                            None
                        })?
                    })
                    .decrypt(&mut bytes[82..(len - 16)], &payload_tag)
                    .map_err(|err| {
                        log::error!("{id} {db_id}, second message payload mac mismatch");
                        self.error = true;
                        err
                    })
                    .ok()?;

                range = 82..(len - 16);
                Some(St::SecondMessage { st, r_epk })
            }
            Some(St::SecondMessage { st, r_epk }) => {
                if bytes.len() < 98 {
                    log::error!(
                        "{id} {db_id}, third message too short {}",
                        hex::encode(bytes)
                    );
                    self.error = true;
                    return None;
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
                    .map_err(|err| {
                        log::error!("{id} {db_id}, third message mac mismatch");
                        self.error = true;
                        err
                    })
                    .ok()?
                    .mix_shared_secret({
                        i_spk = MontgomeryPoint(i_spk_bytes);
                        try_dh(&i_spk, &r_epk, cx).or_else(|| {
                            let i = if self.initiator_is_incoming {
                                "initiator_is_incoming"
                            } else {
                                "initiator_is_outgoing"
                            };
                            log::error!(
                                "{id} {db_id}, {i} secret key not found, r_spk: {}, i_epk: {}",
                                hex::encode(i_spk.as_bytes()),
                                hex::encode(r_epk.as_bytes()),
                            );
                            self.error = true;
                            None
                        })?
                    })
                    .decrypt(&mut bytes[50..(len - 16)], &payload_tag)
                    .map_err(|err| {
                        log::error!("{id} {db_id}, third message payload mac mismatch");
                        self.error = true;
                        err
                    })
                    .ok()?
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
                    log::error!("{id} {db_id}, data slice too short {}", hex::encode(bytes));
                    self.error = true;
                    return None;
                }

                let payload_tag = *GenericArray::from_slice(&bytes[(len - 16)..]);
                let receiver = if id.incoming == self.initiator_is_incoming {
                    &mut initiators_receiver
                } else {
                    &mut responders_receiver
                };
                if receiver
                    .decrypt(&[], &mut bytes[2..(len - 16)], &payload_tag)
                    .is_err()
                {
                    return None;
                } else {
                    range = 2..(len - 16);
                }
                Some(St::Transport {
                    initiators_receiver,
                    responders_receiver,
                })
            }
        };
        Some(&mut bytes[range])
    }
}

#[cfg(test)]
#[test]
#[rustfmt::skip]
fn noise() {
    let mut noise = NoiseState::<super::multistream_select::State<()>>::default();
    let mut cx = Cx {
        db: crate::database::DbFacade::open("/tmp/test_db").unwrap(),
        decrypted: 0,
        failed_to_decrypt: 0,
    };
    cx.db.add_randomness(hex::decode("d1f3bca173136dd555dd97262336ce644a76ec31d521d2befe87caec8678c1a7").expect("valid constant").try_into().expect("valid constant")).unwrap();
    cx.db.add_randomness(hex::decode("1c283e25c80f64f2806d9e19da1a393873d40bdf3d903a3776e013c4fdd97cb3").expect("valid constant").try_into().expect("valid constant")).unwrap();

    let mut id = DirectedId::fake();
    id.incoming = true;
    noise.on_data_(id.clone(), ConnectionId(0), &mut hex::decode("00209844288f8c8f0337dff411d66e0378d950fb7590f9f44d6df969fd59a18ab849").expect("valid constant"), &mut cx).expect("test");
    id.incoming = false;
    noise.on_data_(id.clone(), ConnectionId(0), &mut hex::decode("00c8c0e8867216784ce23e6ad97120c8bfa139941424d0aebcdfe14e339798af4a377f2a97c280a913fdf6a96b4b89c5471a7f4761bec49a557d734b65495eb87e1e00b707d561da835698fe08bab7962b0491751110e8a32a260605a64dbdc18f503958be161fe9546f3c0494c0714f6e57c3eca413cec2d20a483855b4958b96ee79e05f34fa63a74c758ebe9537f4e1c733a7a7ebcd9b1bcc47c2c882ffa361f6ebb404225b60a6bae8e7a6d479d6e1b5c5c1d858ca13dde8cbd285f5bb4d9805578553e3881d5a0d").expect("valid constant"), &mut cx).expect("test");
    id.incoming = true;
    noise.on_data_(id.clone(), ConnectionId(0), &mut hex::decode("00a8e3cfaddd47cf48db1b70b83c15dbdb32bdba21cca65f9f80fb2e7f93d7a82b1b71d6241952e1205d510afad46f8d6d23de1be013618cd79d4e87eec4761292393532e7952bddaeb6709dcb266f861f92ef0eabe282d318f813d11426ac6916240bfead8994c63f10b03f6e241c2b92495a1f63d728fb63ba78e468945f7da081761102465308523dbf50064be4251468abb99db7af8afd71b99100a2fb7a37773a8062d33cc2e1d9").expect("valid constant"), &mut cx).expect("test");
    id.incoming = true;
    noise.on_data_(id.clone(), ConnectionId(0), &mut hex::decode("00375cd2640426acf52810f89147cf5446f8b4bff334c9727c0a45abd220746b2e8b10d269ff28be87c8bb1d53e43e69922ff4b19760ef875d").expect("valid constant"), &mut cx).expect("test");
}

#[cfg(test)]
#[test]
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
