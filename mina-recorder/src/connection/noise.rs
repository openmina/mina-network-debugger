use std::collections::VecDeque;

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

use super::{DirectedId, HandleData, logger::Raw};

type C = (Hmac<Sha256>, Sha256, typenum::B0, ChaCha20Poly1305);

#[derive(Default)]
pub struct State<Inner> {
    machine: Option<St>,
    initiator_is_incoming: bool,
    error: bool,
    inner: Inner,
}

#[allow(dead_code)]
enum St {
    FirstMessage {
        st: SymmetricState<C, ChainingKey<C>>,
        i_epk: MontgomeryPoint,
        i_esk: Scalar,
    },
    SecondMessage {
        st: SymmetricState<C, Key<C, typenum::U1>>,
        i_epk: MontgomeryPoint,
        i_esk: Scalar,
        r_epk: MontgomeryPoint,
        r_esk: Scalar,
        r_spk: MontgomeryPoint,
    },
    Transport {
        initiators_receiver: Cipher<C, 1, false>,
        responders_receiver: Cipher<C, 1, false>,
    },
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData,
{
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], randomness: &mut VecDeque<[u8; 32]>) {
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
            match self.on_data_(id.incoming, bytes, randomness) {
                Ok(bytes) => match msg {
                    Msg::First => (),
                    Msg::Second => {
                        log::info!("{id} responders payload {}", hex::encode(bytes));
                    }
                    Msg::Third => {
                        log::info!("{id} initiators payload {}", hex::encode(bytes));
                    }
                    Msg::Other => self.inner.on_data(id, bytes, randomness),
                },
                Err(bytes) => Raw.on_data(id, bytes, randomness),
            }
        } else {
            Raw.on_data(id, bytes, randomness)
        }
    }
}

impl<Inner> State<Inner> {
    fn on_data_<'a>(
        &mut self,
        incoming: bool,
        bytes: &'a mut [u8],
        randomness: &mut VecDeque<[u8; 32]>,
    ) -> Result<&'a mut [u8], &'a mut [u8]> {
        fn find_sk(pk_bytes: &[u8], randomness: &mut VecDeque<[u8; 32]>) -> Option<Scalar> {
            let (n, sk) = randomness.iter().enumerate().rev().find_map(|(n, rand)| {
                let mut sk_bytes = *rand;
                sk_bytes[0] &= 248;
                sk_bytes[31] &= 127;
                sk_bytes[31] |= 64;
                let sk = Scalar::from_bits(sk_bytes);
                let pk = (&ED25519_BASEPOINT_TABLE * &sk).to_montgomery();
                if pk.as_bytes() == pk_bytes {
                    Some((n, sk))
                } else {
                    None
                }
            })?;
            let _ = (randomness, n);
            // randomness.remove(n);
            Some(sk)
        }

        let range;
        let len = bytes.len();
        self.machine = match self.machine.take() {
            None => {
                self.initiator_is_incoming = incoming;

                let i_epk = MontgomeryPoint(bytes[2..34].try_into().unwrap());
                let i_esk = match find_sk(&bytes[2..34], randomness) {
                    Some(v) => v,
                    None => {
                        self.error = true;
                        return Err(bytes);
                    }
                };
                let st = SymmetricState::new("Noise_XX_25519_ChaChaPoly_SHA256")
                    .mix_hash(&[])
                    .mix_hash(i_epk.as_bytes())
                    .mix_hash(&[]);
                range = 34..len;
                Some(St::FirstMessage { st, i_epk, i_esk })
            }
            Some(St::FirstMessage { st, i_epk, i_esk }) => {
                let r_epk = MontgomeryPoint(bytes[2..34].try_into().unwrap());
                let r_esk = match find_sk(&bytes[2..34], randomness) {
                    Some(v) => v,
                    None => {
                        self.error = true;
                        return Err(bytes);
                    }
                };
                let mut r_spk_bytes: [u8; 32] = bytes[34..66].try_into().unwrap();
                let tag = *GenericArray::from_slice(&bytes[66..82]);
                let r_spk;
                let payload_tag = *GenericArray::from_slice(&bytes[(len - 16)..]);
                let st = st
                    .mix_hash(r_epk.as_bytes())
                    .mix_shared_secret((r_epk * i_esk).to_bytes())
                    .decrypt(&mut r_spk_bytes, &tag)
                    .unwrap()
                    .mix_shared_secret({
                        // let b = MontgomeryPoint(r_spk_bytes).to_edwards(1).unwrap().compress().0;
                        // let pk = libp2p_core::PublicKey::Ed25519(libp2p_core::identity::ed25519::PublicKey::decode(&b).unwrap());
                        // dbg!(libp2p_core::PeerId::from_public_key(&pk));
                        r_spk = MontgomeryPoint(r_spk_bytes);
                        (i_esk * r_spk).to_bytes()
                    })
                    .decrypt(&mut bytes[82..(len - 16)], &payload_tag)
                    .unwrap();

                range = 82..(len - 16);
                Some(St::SecondMessage {
                    st,
                    i_epk,
                    i_esk,
                    r_epk,
                    r_esk,
                    r_spk,
                })
            }
            Some(St::SecondMessage { st, r_esk, .. }) => {
                let mut i_spk_bytes: [u8; 32] = bytes[2..34].try_into().unwrap();
                let tag = *GenericArray::from_slice(&bytes[34..50]);
                let i_spk;
                let payload_tag = *GenericArray::from_slice(&bytes[(len - 16)..]);
                let Output {
                    sender, receiver, ..
                } = st
                    .decrypt(&mut i_spk_bytes, &tag)
                    .unwrap()
                    .mix_shared_secret({
                        // let b = MontgomeryPoint(i_spk_bytes).to_edwards(0).unwrap().compress().0;
                        // let pk = libp2p_core::PublicKey::Ed25519(libp2p_core::identity::ed25519::PublicKey::decode(&b).unwrap());
                        // dbg!(libp2p_core::PeerId::from_public_key(&pk));
                        i_spk = MontgomeryPoint(i_spk_bytes);
                        (i_spk * r_esk).to_bytes()
                    })
                    .decrypt(&mut bytes[50..(len - 16)], &payload_tag)
                    .unwrap()
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
                    self.error = true;
                    return Err(bytes);
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
                    return Err(bytes);
                } else {
                    range = 2..(len - 16);
                }
                Some(St::Transport {
                    initiators_receiver,
                    responders_receiver,
                })
            }
        };
        Ok(&mut bytes[range])
    }
}

#[cfg(test)]
#[test]
#[rustfmt::skip]
fn noise() {
    let mut noise = State::<()>::default();
    let mut randomness = VecDeque::new();
    randomness.push_back(hex::decode("d1f3bca173136dd555dd97262336ce644a76ec31d521d2befe87caec8678c1a7").unwrap().try_into().unwrap());
    randomness.push_back(hex::decode("1c283e25c80f64f2806d9e19da1a393873d40bdf3d903a3776e013c4fdd97cb3").unwrap().try_into().unwrap());

    noise.on_data_(true, &mut hex::decode("00209844288f8c8f0337dff411d66e0378d950fb7590f9f44d6df969fd59a18ab849").unwrap(), &mut randomness).unwrap();
    noise.on_data_(false, &mut hex::decode("00c8c0e8867216784ce23e6ad97120c8bfa139941424d0aebcdfe14e339798af4a377f2a97c280a913fdf6a96b4b89c5471a7f4761bec49a557d734b65495eb87e1e00b707d561da835698fe08bab7962b0491751110e8a32a260605a64dbdc18f503958be161fe9546f3c0494c0714f6e57c3eca413cec2d20a483855b4958b96ee79e05f34fa63a74c758ebe9537f4e1c733a7a7ebcd9b1bcc47c2c882ffa361f6ebb404225b60a6bae8e7a6d479d6e1b5c5c1d858ca13dde8cbd285f5bb4d9805578553e3881d5a0d").unwrap(), &mut randomness).unwrap();
    noise.on_data_(true, &mut hex::decode("00a8e3cfaddd47cf48db1b70b83c15dbdb32bdba21cca65f9f80fb2e7f93d7a82b1b71d6241952e1205d510afad46f8d6d23de1be013618cd79d4e87eec4761292393532e7952bddaeb6709dcb266f861f92ef0eabe282d318f813d11426ac6916240bfead8994c63f10b03f6e241c2b92495a1f63d728fb63ba78e468945f7da081761102465308523dbf50064be4251468abb99db7af8afd71b99100a2fb7a37773a8062d33cc2e1d9").unwrap(), &mut randomness).unwrap();
    noise.on_data_(true, &mut hex::decode("00375cd2640426acf52810f89147cf5446f8b4bff334c9727c0a45abd220746b2e8b10d269ff28be87c8bb1d53e43e69922ff4b19760ef875d").unwrap(), &mut randomness).unwrap();
}
