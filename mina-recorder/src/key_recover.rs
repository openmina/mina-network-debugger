use parking_lot::Mutex;
use sha3::{
    digest::{core_api::XofReaderCoreWrapper, ExtendableOutput, Update, XofReader},
    Shake256, Shake256ReaderCore,
};
use curve25519_dalek::{
    montgomery::MontgomeryPoint, constants::ED25519_BASEPOINT_TABLE, scalar::Scalar,
};

use super::database::{DbCore, RandomnessDatabase};

pub trait KeyDatabase {
    fn reproduced_sk<const EPHEMERAL: bool>(&self, pk: [u8; 32]) -> Option<[u8; 32]>;
}

struct KeyGenerator(XofReaderCoreWrapper<Shake256ReaderCore>);

impl KeyGenerator {
    fn find<F>(&mut self, pk: MontgomeryPoint, cache: F) -> Option<[u8; 32]>
    where
        F: Fn(MontgomeryPoint, Scalar),
    {
        self.take(100)
            .map(|sk| {
                let pk = (&ED25519_BASEPOINT_TABLE * &sk).to_montgomery();
                cache(pk, sk);
                (pk, sk)
            })
            .find(|(this_pk, _)| this_pk.eq(&pk))
            .map(|(_, sk)| sk.to_bytes())
    }
}

impl Iterator for KeyGenerator {
    type Item = Scalar;

    fn next(&mut self) -> Option<Self::Item> {
        let mut sk_bytes = [0; 32];
        self.0.read(&mut sk_bytes);
        log::debug!("sk bytes: {}", hex::encode(sk_bytes));
        sk_bytes[0] &= 248;
        sk_bytes[31] |= 64;
        Some(Scalar::from_bits(sk_bytes))
    }
}

impl KeyGenerator {
    pub fn new(seed: [u8; 32], needle: &[u8]) -> Self {
        Self(Shake256::default().chain(seed).chain(needle).finalize_xof())
    }
}

struct KeyGenerators {
    st: KeyGenerator,
    eph: KeyGenerator,
}

impl KeyGenerators {
    pub fn new(seed: [u8; 32]) -> Self {
        KeyGenerators {
            st: KeyGenerator::new(seed, b"static"),
            eph: KeyGenerator::new(seed, b"ephemeral"),
        }
    }

    pub fn part<const EPHEMERAL: bool>(&mut self) -> &mut KeyGenerator {
        if EPHEMERAL {
            &mut self.eph
        } else {
            &mut self.st
        }
    }
}

pub struct KeyGeneratorWithCache {
    generators: Mutex<Option<KeyGenerators>>,
    db: DbCore,
}

impl KeyGeneratorWithCache {
    pub fn new(db: DbCore) -> Self {
        KeyGeneratorWithCache {
            generators: Mutex::new(None),
            db,
        }
    }
}

impl RandomnessDatabase for KeyGeneratorWithCache {
    fn iterate_randomness<'a>(&'a self) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a> {
        self.db.iterate_randomness()
    }
}

impl KeyDatabase for KeyGeneratorWithCache {
    fn reproduced_sk<const EPHEMERAL: bool>(&self, pk: [u8; 32]) -> Option<[u8; 32]> {
        // try lookup the key
        self.db
            .get_sk(&pk)
            .ok()?
            .and_then(|x| x.try_into().ok())
            .or_else(|| {
                let point = MontgomeryPoint(pk);
                if let Some(mut g) = self.generators.lock().take() {
                    g.part::<EPHEMERAL>()
                        .find(point, |pk, sk| {
                            self.db
                                .put_sk(pk.to_bytes(), sk.to_bytes())
                                .unwrap_or_default()
                        })
                        .inspect(|_| *self.generators.lock() = Some(g))
                } else {
                    // find a seed
                    log::info!("searching seed");
                    self.db
                        .iterate_randomness()
                        .take(8)
                        .filter_map(|x| <[u8; 32]>::try_from(x.to_vec()).ok())
                        .find_map(|seed_candidate| {
                            log::info!("try seed candidate: {}", hex::encode(seed_candidate));
                            KeyGenerators::new(seed_candidate)
                                .part::<EPHEMERAL>()
                                .find(point, |_, _| ())
                                .inspect(|_| {
                                    log::info!("found seed");
                                    *self.generators.lock() =
                                        Some(KeyGenerators::new(seed_candidate));
                                })
                        })
                }
            })
    }
}
