use std::{fmt, mem, iter::Flatten};

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

use super::{DirectedId, HandleData, DynamicProtocol, Cx};

type C = (Hmac<Sha256>, Sha256, typenum::B0, ChaCha20Poly1305);

pub type State<Inner> = ChunkState<NoiseState<Inner>>;

#[derive(Default)]
pub struct ChunkState<Inner> {
    accumulator: Vec<u8>,
    inner: Inner,
}

impl<Inner> DynamicProtocol for ChunkState<Inner>
where
    Inner: Default,
{
    fn from_name(name: &str) -> Self {
        assert_eq!(name, "/noise");
        Self::default()
    }
}

pub enum ChunkOutput<Inner> {
    Nothing,
    Inner(Inner),
}

impl<Inner> fmt::Display for ChunkOutput<Inner>
where
    Inner: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChunkOutput::Nothing => Ok(()),
            ChunkOutput::Inner(inner) => inner.fmt(f),
        }
    }
}

impl<Inner> Iterator for ChunkOutput<Inner>
where
    Inner: Iterator,
{
    type Item = ChunkOutput<Inner::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        match mem::replace(self, ChunkOutput::Nothing) {
            ChunkOutput::Nothing => None,
            ChunkOutput::Inner(mut inner) => {
                let inner_item = inner.next()?;
                *self = ChunkOutput::Inner(inner);
                Some(ChunkOutput::Inner(inner_item))
            }
        }
    }
}

impl<Inner> HandleData for ChunkState<Inner>
where
    Inner: HandleData,
    Inner::Output: IntoIterator,
{
    type Output = ChunkOutput<Flatten<<Vec<Inner::Output> as IntoIterator>::IntoIter>>;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        if self.accumulator.is_empty() && bytes.len() >= 2 {
            let len = u16::from_be_bytes(bytes[..2].try_into().unwrap()) as usize;
            if bytes.len() == 2 + len {
                let inner_out = self.inner.on_data(id, bytes, cx);
                return ChunkOutput::Inner(vec![inner_out].into_iter().flatten());
            }
        }

        self.accumulator.extend_from_slice(bytes);
        let mut inner_outs = vec![];
        loop {
            if self.accumulator.len() >= 2 {
                let len = u16::from_be_bytes(self.accumulator[..2].try_into().unwrap()) as usize;
                if self.accumulator.len() >= 2 + len {
                    let (chunk, remaining) = self.accumulator.split_at_mut(2 + len);
                    let inner_out = self.inner.on_data(id.clone(), chunk, cx);
                    inner_outs.push(inner_out);
                    self.accumulator = remaining.to_vec();
                    continue;
                }
            }
            break;
        }

        if inner_outs.is_empty() {
            ChunkOutput::Nothing
        } else {
            ChunkOutput::Inner(inner_outs.into_iter().flatten())
        }
    }
}

#[derive(Default)]
pub struct NoiseState<Inner> {
    machine: Option<St>,
    initiator_is_incoming: bool,
    error: bool,
    inner: Inner,
}

pub enum NoiseOutput<Inner> {
    Nothing,
    CannotDecrypt(Vec<u8>),
    FirstMessage,
    HandshakePayload(Vec<u8>),
    Inner(Inner),
}

impl<Inner> fmt::Display for NoiseOutput<Inner>
where
    Inner: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NoiseOutput::Nothing => Ok(()),
            NoiseOutput::CannotDecrypt(bytes) => write!(f, "cannot decrypt {}", hex::encode(bytes)),
            NoiseOutput::FirstMessage => write!(f, "handshake"),
            NoiseOutput::HandshakePayload(p) => write!(f, "handshake {}", hex::encode(p)),
            NoiseOutput::Inner(inner) => inner.fmt(f),
        }
    }
}

impl<Inner> Iterator for NoiseOutput<Inner>
where
    Inner: Iterator,
{
    type Item = NoiseOutput<Inner::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        match mem::replace(self, NoiseOutput::Nothing) {
            NoiseOutput::Nothing => None,
            NoiseOutput::CannotDecrypt(bytes) => Some(NoiseOutput::CannotDecrypt(bytes)),
            NoiseOutput::FirstMessage => Some(NoiseOutput::FirstMessage),
            NoiseOutput::HandshakePayload(payload) => Some(NoiseOutput::HandshakePayload(payload)),
            NoiseOutput::Inner(mut inner) => {
                let inner_item = inner.next()?;
                *self = NoiseOutput::Inner(inner);
                Some(NoiseOutput::Inner(inner_item))
            }
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
    Inner::Output: IntoIterator,
{
    type Output = NoiseOutput<<Inner::Output as IntoIterator>::IntoIter>;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
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
            match self.on_data_(id.incoming, bytes, cx) {
                Some(bytes) => match msg {
                    Msg::First => NoiseOutput::FirstMessage,
                    Msg::Second => NoiseOutput::HandshakePayload(bytes.to_vec()),
                    Msg::Third => NoiseOutput::HandshakePayload(bytes.to_vec()),
                    Msg::Other => NoiseOutput::Inner(self.inner.on_data(id, bytes, cx).into_iter()),
                },
                None => {
                    // Raw.on_data(id, bytes, cx);
                    NoiseOutput::CannotDecrypt(bytes.to_vec())
                }
            }
        } else {
            // Raw.on_data(id, bytes, cx);
            NoiseOutput::CannotDecrypt(bytes.to_vec())
        }
    }
}

impl<Inner> NoiseState<Inner> {
    fn on_data_<'a>(
        &mut self,
        incoming: bool,
        bytes: &'a mut [u8],
        cx: &mut Cx,
    ) -> Option<&'a mut [u8]> {
        fn find_sk(pk: &MontgomeryPoint, cx: &Cx) -> Option<Scalar> {
            let sk = cx.iter_rand().find_map(|rand| {
                let mut sk_bytes = *rand;
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
                self.initiator_is_incoming = incoming;

                if bytes.len() < 34 {
                    self.error = true;
                    return None;
                }

                let i_epk = MontgomeryPoint(bytes[2..34].try_into().unwrap());
                let st = SymmetricState::new("Noise_XX_25519_ChaChaPoly_SHA256")
                    .mix_hash(&[])
                    .mix_hash(i_epk.as_bytes())
                    .mix_hash(&[]);
                range = 34..len;
                Some(St::FirstMessage { st, i_epk })
            }
            Some(St::FirstMessage { st, i_epk }) => {
                if bytes.len() < 98 {
                    self.error = true;
                    return None;
                }

                let r_epk = MontgomeryPoint(bytes[2..34].try_into().unwrap());
                let ee = try_dh(&r_epk, &i_epk, cx).or_else(|| {
                    self.error = true;
                    None
                })?;

                let mut r_spk_bytes: [u8; 32] = bytes[34..66].try_into().unwrap();
                let tag = *GenericArray::from_slice(&bytes[66..82]);
                let r_spk;
                let payload_tag = *GenericArray::from_slice(&bytes[(len - 16)..]);
                let st = st
                    .mix_hash(r_epk.as_bytes())
                    .mix_shared_secret(ee)
                    .decrypt(&mut r_spk_bytes, &tag)
                    .unwrap()
                    .mix_shared_secret({
                        // let b = MontgomeryPoint(r_spk_bytes).to_edwards(1).unwrap().compress().0;
                        // let pk = libp2p_core::PublicKey::Ed25519(libp2p_core::identity::ed25519::PublicKey::decode(&b).unwrap());
                        // dbg!(libp2p_core::PeerId::from_public_key(&pk));
                        r_spk = MontgomeryPoint(r_spk_bytes);
                        try_dh(&r_spk, &i_epk, cx).or_else(|| {
                            self.error = true;
                            None
                        })?
                    })
                    .decrypt(&mut bytes[82..(len - 16)], &payload_tag)
                    .unwrap();

                range = 82..(len - 16);
                Some(St::SecondMessage { st, r_epk })
            }
            Some(St::SecondMessage { st, r_epk }) => {
                if bytes.len() < 98 {
                    self.error = true;
                    return None;
                }

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
                        try_dh(&i_spk, &r_epk, cx).or_else(|| {
                            self.error = true;
                            None
                        })?
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
                    return None;
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
    let mut noise = NoiseState::<()>::default();
    let mut cx = Cx::default();
    cx.push_randomness(hex::decode("d1f3bca173136dd555dd97262336ce644a76ec31d521d2befe87caec8678c1a7").unwrap().try_into().unwrap());
    cx.push_randomness(hex::decode("1c283e25c80f64f2806d9e19da1a393873d40bdf3d903a3776e013c4fdd97cb3").unwrap().try_into().unwrap());

    noise.on_data_(true, &mut hex::decode("00209844288f8c8f0337dff411d66e0378d950fb7590f9f44d6df969fd59a18ab849").unwrap(), &mut cx).unwrap();
    noise.on_data_(false, &mut hex::decode("00c8c0e8867216784ce23e6ad97120c8bfa139941424d0aebcdfe14e339798af4a377f2a97c280a913fdf6a96b4b89c5471a7f4761bec49a557d734b65495eb87e1e00b707d561da835698fe08bab7962b0491751110e8a32a260605a64dbdc18f503958be161fe9546f3c0494c0714f6e57c3eca413cec2d20a483855b4958b96ee79e05f34fa63a74c758ebe9537f4e1c733a7a7ebcd9b1bcc47c2c882ffa361f6ebb404225b60a6bae8e7a6d479d6e1b5c5c1d858ca13dde8cbd285f5bb4d9805578553e3881d5a0d").unwrap(), &mut cx).unwrap();
    noise.on_data_(true, &mut hex::decode("00a8e3cfaddd47cf48db1b70b83c15dbdb32bdba21cca65f9f80fb2e7f93d7a82b1b71d6241952e1205d510afad46f8d6d23de1be013618cd79d4e87eec4761292393532e7952bddaeb6709dcb266f861f92ef0eabe282d318f813d11426ac6916240bfead8994c63f10b03f6e241c2b92495a1f63d728fb63ba78e468945f7da081761102465308523dbf50064be4251468abb99db7af8afd71b99100a2fb7a37773a8062d33cc2e1d9").unwrap(), &mut cx).unwrap();
    noise.on_data_(true, &mut hex::decode("00375cd2640426acf52810f89147cf5446f8b4bff334c9727c0a45abd220746b2e8b10d269ff28be87c8bb1d53e43e69922ff4b19760ef875d").unwrap(), &mut cx).unwrap();
}
