use std::{collections::BTreeMap, task::Poll};

use crate::database::StreamKind;

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult, StreamId};

pub struct State<Inner> {
    incoming: acc::State<true>,
    outgoing: acc::State<false>,
    error: bool,
    #[allow(dead_code)]
    inners: BTreeMap<StreamId, Status<Inner>>,
}

#[allow(dead_code)]
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
                u16::from_be_bytes(x)
            };

            if flags >= 16 {
                return Err(HeaderParseError::Flags(flags));
            }

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
            v[2..4].clone_from_slice(&flags.to_be_bytes());
            v[4..8].clone_from_slice(&stream_id.to_be_bytes());

            v
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct Header {
        pub version: u8,
        pub ty: HeaderType,
        pub flags: u16,
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
pub use self::header::{HeaderParseError, Header, HeaderType, YamuxError};

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
                if bytes.len() >= offset {
                    let header_bytes = <[u8; 12]>::try_from(&bytes[..offset]).expect("cannot fail");
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
        pub fn accumulate<'a>(&mut self, bytes: &'a [u8]) -> Poll<Output> {
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
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _cx: &mut Cx, db: &Db) -> DbResult<()> {
        if self.error {
            // TODO: report
            return Ok(());
        }

        for result in self.process(id.incoming, bytes) {
            match result {
                Err(err) => {
                    self.error = true;
                    // TODO: report
                    log::error!("{id} {err}");
                    return Ok(());
                }
                Ok(acc::Output { header, bytes }) => {
                    // TODO:
                    let _ = bytes;
                    let stream_id = if header.stream_id == 0 {
                        StreamId::Handshake
                    } else if header.stream_id % 2 == 0 {
                        StreamId::Forward((header.stream_id / 2) as u64)
                    } else {
                        StreamId::Backward((header.stream_id / 2) as u64)
                    };
                    let db_stream = db.get(stream_id);
                    let header_bytes = <[u8; 12]>::from(&header);
                    db_stream.add(&id, StreamKind::Yamux, &header_bytes)?;
                }
            }
        }

        Ok(())
    }
}
