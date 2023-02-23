use std::{collections::BTreeMap, io, ops::AddAssign};

use capnp::{
    message::{Builder, Reader},
    serialize::{self, OwnedSegments},
};

use serde::{Deserialize, Serialize};

use super::CapnpEncode;

#[derive(Default, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ChecksumTotal {
    pub ipc: ChecksumPair,
    pub network: BTreeMap<String, ChecksumPair>,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ChecksumPair(pub Checksum, pub Checksum);

impl ChecksumPair {
    pub fn matches(&self, other: &Self) -> bool {
        self.0.matches(&other.1) && self.1.matches(&other.0)
    }

    pub fn matches_(&self, other: &Self) -> bool {
        self.0.matches(&other.0) && self.1.matches(&other.1)
    }

    pub fn bytes_number(&self) -> u64 {
        self.0.len + self.1.len
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Checksum {
    crc_pos: usize,
    crc: [u64; Self::NUM],
    #[cfg(feature = "dump")]
    #[serde(serialize_with = "serialize_hex")]
    #[serde(deserialize_with = "deserialize_hex")]
    data: Vec<u8>,
    len: u64,
    cnt: u64,
}

#[cfg(feature = "dump")]
fn deserialize_hex<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    hex::decode(String::deserialize(deserializer)?).map_err(<D::Error as serde::de::Error>::custom)
}

#[cfg(feature = "dump")]
fn serialize_hex<S>(value: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    hex::encode(value).serialize(serializer)
}

impl Checksum {
    const NUM: usize = 4;

    pub fn matches(&self, other: &Self) -> bool {
        self.crc
            .into_iter()
            .any(|l_crc| other.crc.into_iter().any(|r_crc| l_crc == r_crc))
    }
}

impl<'a> AddAssign<&'a [u8]> for Checksum {
    fn add_assign(&mut self, data: &'a [u8]) {
        let new_pos = (self.crc_pos + 1) % Self::NUM;
        self.crc[new_pos] = crc64::crc64(self.crc[self.crc_pos], data);
        self.crc_pos = new_pos;
        self.len += data.len() as u64;
        self.cnt += 1;
        #[cfg(feature = "dump")]
        self.data.extend_from_slice(data);
    }
}

pub struct ChecksumIo<T> {
    pub inner: T,
    checksum: Checksum,
    buffer: Vec<u8>,
}

impl<T> ChecksumIo<T> {
    pub fn new(inner: T) -> Self {
        ChecksumIo {
            inner,
            checksum: Checksum::default(),
            buffer: vec![],
        }
    }

    pub fn checksum(&self) -> Checksum {
        self.checksum.clone()
    }

    fn reset(&mut self) {
        self.checksum += &self.buffer;
        self.buffer.clear();
    }
}

impl<R> ChecksumIo<R>
where
    R: io::Read,
{
    pub fn decode<T>(&mut self) -> capnp::Result<T>
    where
        T: TryFrom<Reader<OwnedSegments>, Error = capnp::Error>,
        R: io::Read,
    {
        let mut s = self;
        let reader = serialize::read_message(&mut s, Default::default())?;
        s.reset();
        T::try_from(reader)
    }
}

impl<W> ChecksumIo<W>
where
    W: io::Write,
{
    pub fn encode<T>(&mut self, value: &T) -> capnp::Result<()>
    where
        T: for<'a> CapnpEncode<'a>,
        W: io::Write,
    {
        let mut s = self;
        let mut message = Builder::new_default();
        value.build(message.init_root())?;
        serialize::write_message(&mut s, &message).map(|()| s.reset())
    }
}

impl<R> io::Read for ChecksumIo<R>
where
    R: io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf).map(|len| {
            self.buffer.extend_from_slice(&buf[..len]);
            len
        })
    }
}

impl<W> io::Write for ChecksumIo<W>
where
    W: io::Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf).map(|len| {
            self.buffer.extend_from_slice(&buf[..len]);
            len
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
