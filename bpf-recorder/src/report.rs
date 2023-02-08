use std::{ops::AddAssign, time::SystemTime, net::IpAddr, collections::BTreeMap};

use serde::{Serialize, Deserialize};

#[derive(Default, Serialize, Deserialize)]
pub struct Report {
    pub version: String,
    pub ipc: ChecksumPair,
    pub network: BTreeMap<IpAddr, Connection>,
}

impl Report {
    pub fn new(version: String) -> Self {
        Report {
            version,
            ..Default::default()
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Connection {
    pub incoming: bool,
    pub fd: i32,
    pub checksum: ChecksumPair,
    pub timestamp: SystemTime,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ChecksumPair(pub Checksum, pub Checksum);

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Checksum {
    crc_pos: usize,
    crc: [u64; Self::NUM],
    len: u64,
    cnt: u64,
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
