use std::{collections::BTreeMap, io::Cursor, borrow::Cow};

use binprot::{BinProtRead, BinProtWrite};
use mina_p2p_messages::{
    string::CharString as BString,
    rpc_kernel::{QueryHeader, MessageHeader, ResponseHeader},
    utils,
};
use thiserror::Error;

use super::accumulator;

#[derive(Default)]
pub struct State {
    acc: accumulator::State,
    pending: BTreeMap<i64, Header>,
}

struct Header {
    tag: BString,
    version: i32,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("response {id} without request")]
    ResponseWithoutRequest { id: i64 },
}

impl State {
    // u64 in little endian
    fn decode_size(bytes: &[u8]) -> Option<(usize, usize)> {
        let mut s = Cursor::new(bytes);
        let l1 = utils::stream_decode_size(&mut s).ok()?;
        let l0 = s.position() as usize;
        Some((l0, l1))
    }

    pub fn extend<'a>(&mut self, bytes: &'a mut [u8]) -> Result<Option<Cow<'a, [u8]>>, Error> {
        if self.acc.extend(Self::decode_size, bytes) {
            Ok(None)
        } else {
            self.post_process(bytes)
        }
    }

    pub fn next_msg(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let mut msg = match self.acc.next_msg(Self::decode_size) {
            Some(v) => v.to_vec(),
            None => return Ok(None),
        };
        self.post_process(&mut msg).map(|x| x.map(|c| c.to_vec()))
    }

    fn post_process<'a>(&mut self, bytes: &'a mut [u8]) -> Result<Option<Cow<'a, [u8]>>, Error> {
        let (l0, _) = Self::decode_size(bytes).unwrap();
        let mut stream = Cursor::new(&mut bytes[l0..]);
        match MessageHeader::binprot_read(&mut stream) {
            Err(err) => {
                log::error!("{err}");
                Ok(None)
            }
            Ok(MessageHeader::Heartbeat) => Ok(None),
            Ok(MessageHeader::Query(QueryHeader { tag, version, id })) => {
                let header = Header { tag, version };
                self.pending.insert(id, header);
                Ok(Some(Cow::Borrowed(bytes)))
            }
            Ok(MessageHeader::Response(ResponseHeader { id })) => {
                if let Some(Header { tag, version }) = self.pending.remove(&id) {
                    let q = QueryHeader { tag, version, id };
                    let mut b = [0; 8].to_vec();
                    b.push(2);
                    q.binprot_write(&mut b).unwrap();
                    let remaining = {
                        let len = stream.position().min(stream.get_ref().len() as u64);
                        &stream.get_ref()[(len as usize)..]
                    };
                    b.extend_from_slice(remaining);
                    let new_len = b.len() - l0;
                    b[0..8].clone_from_slice(&(new_len as u64).to_le_bytes());

                    Ok(Some(Cow::Owned(b)))
                } else if id != 4411474 {
                    Err(Error::ResponseWithoutRequest { id })
                } else {
                    Ok(None)
                }
            }
        }
    }
}
