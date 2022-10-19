use std::{fmt, time::SystemTime, io};

use radiation::{Emit, Absorb, AbsorbExt};
use thiserror::Error;

use crate::custom_coding;

#[derive(Absorb, Emit)]
pub struct ChunkHeader {
    pub size: u32,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub time: SystemTime,
    pub encryption_status: EncryptionStatus,
    pub incoming: bool,
}

#[derive(Clone, Debug, Absorb, Emit)]
#[tag(u8)]
pub enum EncryptionStatus {
    #[tag(1)]
    Raw,
    #[tag(0xff)]
    DecryptedPnet,
    #[tag(0)]
    DecryptedNoise,
}

impl ChunkHeader {
    pub const SIZE: usize = 18; // size 4 + time 12 + encrypted 1 + incoming 1
}

impl fmt::Display for ChunkHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use time::OffsetDateTime;

        let (hour, minute, second, nano) = OffsetDateTime::from(self.time).time().as_hms_nano();
        let incoming = if self.incoming {
            "incoming"
        } else {
            "outgoing"
        };
        let status = &self.encryption_status;

        write!(
            f,
            "{hour:02}:{minute:02}:{second:02}.{nano:09} {status:?} {incoming}"
        )
    }
}

#[derive(Debug, Error)]
pub struct ChunkParser<R>(R);

impl<R> ChunkParser<R>
where
    R: io::Read,
{
    pub fn new(inner: R) -> Self {
        ChunkParser(inner)
    }
}

impl<R> Iterator for ChunkParser<R>
where
    R: io::Read,
{
    type Item = (ChunkHeader, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        let mut header_bytes = vec![0; ChunkHeader::SIZE];
        self.0.read_exact(&mut header_bytes).ok()?;
        let header = ChunkHeader::absorb_ext(&header_bytes).ok()?;
        let mut data = vec![0; header.size as usize];
        self.0.read_exact(&mut data).ok()?;
        Some((header, data))
    }
}
