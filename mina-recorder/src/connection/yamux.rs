use std::{
    collections::{BTreeMap, VecDeque},
    task::Poll,
};

use crate::database::StreamKind;

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult, StreamId};

pub struct State<Inner> {
    incoming: acc::State<true>,
    outgoing: acc::State<false>,
    error: bool,
    inners: BTreeMap<StreamId, Status<Inner>>,
    recent_reset: VecDeque<StreamId>,
}

pub enum Status<Inner> {
    Duplex(Inner),
    IncomingOnly(Inner),
    OutgoingOnly(Inner),
}

impl<Inner> AsMut<Inner> for Status<Inner> {
    fn as_mut(&mut self) -> &mut Inner {
        match self {
            Status::Duplex(inner) => inner,
            Status::IncomingOnly(inner) => inner,
            Status::OutgoingOnly(inner) => inner,
        }
    }
}

impl<Inner> DynamicProtocol for State<Inner> {
    fn from_name(name: &str, _: StreamId) -> Self {
        assert_eq!(name, "/coda/yamux/1.0.0");
        State {
            incoming: acc::State::default(),
            outgoing: acc::State::default(),
            error: false,
            inners: BTreeMap::new(),
            recent_reset: VecDeque::with_capacity(512),
        }
    }
}

mod header {
    use serde::{Serialize, Deserialize};
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum HeaderParseError {
        #[error("the version {0} is not supported")]
        NotSupportedVersion(u8),
        #[error("unknown type {0}")]
        Type(u8),
        #[error("unknown flags {0}")]
        Flags(u16),
        #[error("unknown error code {0}")]
        UnknownErrorCode(u32),
    }

    bitflags::bitflags! {
        #[derive(Serialize, Deserialize)]
        pub struct HeaderFlags: u16 {
            const SYN = 0b0001;
            const ACK = 0b0010;
            const FIN = 0b0100;
            const RST = 0b1000;
        }
    }

    impl TryFrom<[u8; 12]> for Header {
        type Error = HeaderParseError;

        fn try_from(v: [u8; 12]) -> Result<Self, Self::Error> {
            let version = v[0];
            if version != 0 {
                return Err(HeaderParseError::NotSupportedVersion(version));
            }

            let last = {
                let mut x = [0; 4];
                x.clone_from_slice(&v[8..]);
                x
            };

            let ty = v[1];
            let ty = match ty {
                0 => {
                    let length = u32::from_be_bytes(last);
                    HeaderType::Data { length }
                }
                1 => {
                    let delta = i32::from_be_bytes(last);
                    HeaderType::WindowUpdate { delta }
                }
                2 => {
                    let opaque = u32::from_be_bytes(last);
                    HeaderType::Ping { opaque }
                }
                3 => {
                    let code = u32::from_be_bytes(last);
                    let result = match code {
                        0 => Ok(()),
                        1 => Err(YamuxError::Protocol),
                        2 => Err(YamuxError::Internal),
                        c => return Err(HeaderParseError::UnknownErrorCode(c)),
                    };
                    HeaderType::GoAway(result)
                }
                t => return Err(HeaderParseError::Type(t)),
            };

            let flags = {
                let mut x = [0; 2];
                x.clone_from_slice(&v[2..4]);
                let bits = u16::from_be_bytes(x);
                match HeaderFlags::from_bits(bits) {
                    Some(v) => v,
                    None => return Err(HeaderParseError::Flags(bits)),
                }
            };

            let stream_id = {
                let mut x = [0; 4];
                x.clone_from_slice(&v[4..8]);
                u32::from_be_bytes(x)
            };

            Ok(Header {
                version,
                ty,
                flags,
                stream_id,
            })
        }
    }

    impl<'a> From<&'a Header> for [u8; 12] {
        fn from(
            Header {
                version,
                ty,
                flags,
                stream_id,
            }: &'a Header,
        ) -> Self {
            let mut v = [0; 12];
            v[0] = *version;
            match ty {
                HeaderType::Data { length } => {
                    v[1] = 0;
                    v[8..].clone_from_slice(&length.to_be_bytes());
                }
                HeaderType::WindowUpdate { delta } => {
                    v[1] = 1;
                    v[8..].clone_from_slice(&delta.to_be_bytes());
                }
                HeaderType::Ping { opaque } => {
                    v[1] = 2;
                    v[8..].clone_from_slice(&opaque.to_be_bytes());
                }
                HeaderType::GoAway(Ok(())) => {
                    v[1] = 3;
                    v[8..].clone_from_slice(&0u32.to_be_bytes());
                }
                HeaderType::GoAway(Err(YamuxError::Protocol)) => {
                    v[1] = 3;
                    v[8..].clone_from_slice(&1u32.to_be_bytes());
                }
                HeaderType::GoAway(Err(YamuxError::Internal)) => {
                    v[1] = 3;
                    v[8..].clone_from_slice(&2u32.to_be_bytes());
                }
            }
            v[2..4].clone_from_slice(&flags.bits().to_be_bytes());
            v[4..8].clone_from_slice(&stream_id.to_be_bytes());

            v
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct Header {
        pub version: u8,
        pub ty: HeaderType,
        pub flags: HeaderFlags,
        pub stream_id: u32,
    }

    impl Header {
        pub fn payload_length(&self) -> usize {
            match &self.ty {
                HeaderType::Data { length } => *length as usize,
                _ => 0,
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    #[serde(tag = "type")]
    pub enum HeaderType {
        Data { length: u32 },
        WindowUpdate { delta: i32 },
        Ping { opaque: u32 },
        GoAway(Result<(), YamuxError>),
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(rename_all = "snake_case")]
    pub enum YamuxError {
        Protocol,
        Internal,
    }
}
pub use self::header::{HeaderParseError, Header, HeaderType, HeaderFlags, YamuxError};

// TODO: reuse `mplex::acc`
mod acc {
    use std::{borrow::Cow, task::Poll, cmp::Ordering, mem};

    use thiserror::Error;

    use super::{Header, HeaderParseError};

    #[derive(Default)]
    pub struct State<const INCOMING: bool> {
        acc: Vec<u8>,
    }

    #[derive(Debug)]
    pub struct Output<'a> {
        pub header: Header,
        pub bytes: Cow<'a, [u8]>,
    }

    #[derive(Debug, Error)]
    pub enum Error {
        #[error("header parse error: {0}")]
        HeaderParse(HeaderParseError),
    }

    impl<const INCOMING: bool> State<INCOMING> {
        pub fn accumulate<'a>(&mut self, bytes: &'a [u8]) -> Poll<Result<Output<'a>, Error>> {
            let offset = 12;

            if self.acc.is_empty() {
                if bytes.len() >= offset {
                    let header_bytes = <[u8; 12]>::try_from(&bytes[..offset]).expect("cannot fail");
                    let header = match Header::try_from(header_bytes) {
                        Ok(v) => v,
                        Err(err) => return Poll::Ready(Err(Error::HeaderParse(err))),
                    };
                    let end = offset + header.payload_length();
                    match bytes.len().cmp(&end) {
                        Ordering::Less => self.acc.extend_from_slice(bytes),
                        // good case, accumulator is empty
                        // and `bytes` contains exactly whole message
                        Ordering::Equal => {
                            return Poll::Ready(Ok(Output {
                                header,
                                bytes: Cow::Borrowed(&bytes[offset..]),
                            }));
                        }
                        Ordering::Greater => {
                            let (bytes, remaining) = bytes.split_at(end);
                            self.acc = remaining.to_vec();
                            return Poll::Ready(Ok(Output {
                                header,
                                bytes: Cow::Borrowed(&bytes[offset..]),
                            }));
                        }
                    }
                } else {
                    self.acc.extend_from_slice(bytes);
                }
            } else {
                self.acc.extend_from_slice(bytes);
                if self.acc.len() >= offset {
                    let header_bytes =
                        <[u8; 12]>::try_from(&self.acc[..offset]).expect("cannot fail");
                    let header = match Header::try_from(header_bytes) {
                        Ok(v) => v,
                        Err(err) => return Poll::Ready(Err(Error::HeaderParse(err))),
                    };
                    let end = offset + header.payload_length();
                    match self.acc.len().cmp(&end) {
                        Ordering::Less => (),
                        // not bad case, accumulator contains exactly whole message
                        Ordering::Equal => {
                            let acc = mem::take(&mut self.acc);

                            return Poll::Ready(Ok(Output {
                                header,
                                bytes: Cow::Owned(acc[offset..].to_vec()),
                            }));
                        }
                        Ordering::Greater => {
                            let bytes = self.acc[offset..end].to_vec();
                            self.acc = self.acc[end..].to_vec();
                            return Poll::Ready(Ok(Output {
                                header,
                                bytes: Cow::Owned(bytes),
                            }));
                        }
                    }
                }
            }

            Poll::Pending
        }
    }
}

#[allow(dead_code)]
mod acc_next {
    use std::{task::Poll, mem};

    use super::{Header, acc};

    pub struct Output {
        header: Header,
        msg: Vec<u8>,
        error: Option<String>,
    }

    #[derive(Default)]
    pub struct State<const INCOMING: bool> {
        inner: acc::State<INCOMING>,
        header: Option<Header>,
        acc: Vec<u8>,
    }

    impl<const INCOMING: bool> State<INCOMING> {
        pub fn accumulate(&mut self, bytes: &[u8]) -> Poll<Output> {
            if let Some(header) = &self.header {
                self.acc.extend_from_slice(bytes);
                if self.acc.len() >= header.payload_length() {
                    return Poll::Ready(Output {
                        header: self.header.take().unwrap(),
                        msg: mem::take(&mut self.acc),
                        error: None,
                    });
                }
            }

            unimplemented!()
        }
    }
}

impl<Inner> State<Inner>
where
    Inner: From<StreamId>,
{
    fn process<'a>(
        &mut self,
        incoming: bool,
        bytes: &'a [u8],
    ) -> Vec<Result<acc::Output<'a>, acc::Error>> {
        let mut output = vec![];
        let mut bytes = bytes;
        loop {
            let acc = if incoming {
                self.incoming.accumulate(bytes)
            } else {
                self.outgoing.accumulate(bytes)
            };
            bytes = &[];
            if let Poll::Ready(result) = acc {
                if let Ok(out) = &result {
                    let header = &out.header;
                    let stream_id = if header.stream_id == 0 {
                        StreamId::Handshake
                    } else if header.stream_id % 2 == 0 {
                        StreamId::Forward((header.stream_id / 2) as u64)
                    } else {
                        StreamId::Backward((header.stream_id / 2) as u64)
                    };

                    if header.flags.contains(HeaderFlags::SYN) {
                        let stream = Inner::from(stream_id);
                        self.inners.insert(stream_id, Status::Duplex(stream));
                    } else if out.header.flags.contains(HeaderFlags::FIN) {
                        let error = match (self.inners.remove(&stream_id), incoming) {
                            (Some(Status::Duplex(stream)), true) => {
                                self.inners.insert(stream_id, Status::OutgoingOnly(stream));
                                false
                            }
                            (Some(Status::Duplex(stream)), false) => {
                                self.inners.insert(stream_id, Status::IncomingOnly(stream));
                                false
                            }
                            (Some(Status::IncomingOnly(_)), true) => false,
                            (Some(Status::OutgoingOnly(_)), false) => false,
                            (Some(Status::OutgoingOnly(_)), true) => true,
                            (Some(Status::IncomingOnly(_)), false) => true,
                            (None, _) => true,
                        };
                        // TODO: report
                        let _ = error;
                    } else if out.header.flags.contains(HeaderFlags::RST) {
                        if self.recent_reset.len() == 512 {
                            self.recent_reset.pop_front();
                        }
                        self.recent_reset.push_back(stream_id);
                        self.inners.remove(&stream_id);
                    }
                }
                output.push(result);
            } else {
                break;
            }
        }

        output
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + From<StreamId>,
{
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &Cx, db: &mut Db) -> DbResult<()> {
        if self.error {
            // TODO: report
            return Ok(());
        }

        for result in self.process(id.incoming, bytes) {
            match result {
                Err(err) => {
                    self.error = true;
                    // TODO: report
                    log::error!("{id} {} {err}", db.id());
                    return Ok(());
                }
                Ok(acc::Output { header, mut bytes }) => {
                    let stream_id = if header.stream_id == 0 {
                        StreamId::Handshake
                    } else if header.stream_id % 2 == 0 {
                        StreamId::Forward((header.stream_id / 2) as u64)
                    } else {
                        StreamId::Backward((header.stream_id / 2) as u64)
                    };
                    let db_stream = db.get(stream_id);
                    if let HeaderType::Data { .. } = &header.ty {
                        if let Some(s) = self.inners.get_mut(&stream_id) {
                            s.as_mut().on_data(id.clone(), bytes.to_mut(), cx, db)?;
                        } else if !self.recent_reset.contains(&stream_id) {
                            log::warn!("{id} {} doesn't exist {stream_id}", db.id());
                        }
                    } else {
                        let header_bytes = <[u8; 12]>::from(&header);
                        db_stream.add(&id, StreamKind::Yamux, &header_bytes)?;
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::database::StreamId;

    use super::{State, DynamicProtocol};

    #[test]
    fn trivial_acc() {
        let mut st = State::<StreamId>::from_name("/coda/yamux/1.0.0", StreamId::Handshake);

        let data = hex::decode("000000000000001100000010ffffffff").unwrap();
        let r = st.process(true, &data);
        assert!(r.is_empty());

        let data = hex::decode("ffffffffffffffffffffffff").unwrap();
        let r = st.process(true, &data);

        let output = r[0].as_ref().unwrap();
        assert_eq!(output.header.payload_length(), 16);
        assert_eq!(output.bytes.as_ref(), [0xff; 16]);
    }
}
