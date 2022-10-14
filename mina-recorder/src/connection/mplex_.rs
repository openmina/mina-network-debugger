use std::{collections::BTreeMap, borrow::Cow, task::Poll, fmt};

use crate::database::{DbStream, StreamId, StreamKind};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

pub struct State<Inner> {
    incoming: acc::State<true>,
    outgoing: acc::State<false>,
    stream: Option<DbStream>,
    inners: BTreeMap<StreamId, Status<Inner>>,
}

impl<Inner> DynamicProtocol for State<Inner> {
    fn from_name(name: &str, _: u64, _: bool) -> Self {
        assert_eq!(name, "/coda/mplex/1.0.0");
        State {
            incoming: acc::State::default(),
            outgoing: acc::State::default(),
            stream: None,
            inners: BTreeMap::default(),
        }
    }
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

mod acc {
    use std::{borrow::Cow, task::Poll, cmp::Ordering, mem};

    use unsigned_varint::decode;

    use super::StreamId;

    pub enum Tag {
        New,
        Msg,
        Close,
        Reset,
    }

    #[derive(Default)]
    pub struct State<const INCOMING: bool> {
        acc: Vec<u8>,
    }

    pub struct Output<'a> {
        pub tag: Tag,
        pub stream_id: StreamId,
        pub bytes: Cow<'a, [u8]>,
    }

    impl<const INCOMING: bool> State<INCOMING> {
        pub fn accumulate<'a>(&mut self, bytes: &'a [u8]) -> Poll<Output<'a>> {
            let r = |bytes: &[u8]| -> Option<(Tag, StreamId, usize, usize)> {
                let (v, remaining) = decode::u64(bytes).ok()?;

                let initiator = v % 2 == 0;

                let tag = match v & 7 {
                    0 => Tag::New,
                    1 | 2 => Tag::Msg,
                    3 | 4 => Tag::Close,
                    5 | 6 => Tag::Reset,
                    7 => panic!("wrong header tag"),
                    _ => unreachable!(),
                };

                let stream_id = if initiator == INCOMING {
                    StreamId::Forward(v >> 3)
                } else {
                    StreamId::Backward(v >> 3)
                };

                let (len, remaining) = decode::u64(remaining).ok()?;
                let offset = remaining.as_ptr() as usize - bytes.as_ptr() as usize;
                Some((tag, stream_id, len as usize, offset))
            };

            if self.acc.is_empty() {
                if let Some((tag, stream_id, len, offset)) = r(bytes) {
                    let end = offset + len;
                    match bytes.len().cmp(&end) {
                        Ordering::Less => self.acc.extend_from_slice(bytes),
                        // good case, accumulator is empty
                        // and `bytes` contains exactly whole message
                        Ordering::Equal => {
                            return Poll::Ready(Output {
                                tag,
                                stream_id,
                                bytes: Cow::Borrowed(&bytes[offset..]),
                            });
                        }
                        Ordering::Greater => {
                            let (bytes, remaining) = bytes.split_at(end);
                            self.acc = remaining.to_vec();
                            return Poll::Ready(Output {
                                tag,
                                stream_id,
                                bytes: Cow::Borrowed(&bytes[offset..]),
                            });
                        }
                    }
                } else {
                    self.acc.extend_from_slice(bytes);
                }
            } else {
                self.acc.extend_from_slice(bytes);
                if let Some((tag, stream_id, len, offset)) = r(&self.acc) {
                    let end = offset + len;
                    match self.acc.len().cmp(&end) {
                        Ordering::Less => (),
                        // not bad case, accumulator contains exactly whole message
                        Ordering::Equal => {
                            return Poll::Ready(Output {
                                tag,
                                stream_id,
                                bytes: Cow::Owned(mem::take(&mut self.acc)),
                            });
                        }
                        Ordering::Greater => {
                            let bytes = self.acc[offset..end].to_vec();
                            self.acc = self.acc[end..].to_vec();
                            return Poll::Ready(Output {
                                tag,
                                stream_id,
                                bytes: Cow::Owned(bytes),
                            });
                        }
                    }
                }
            }

            Poll::Pending
        }
    }
}

struct Output<'a> {
    stream_id: StreamId,
    variant: OutputVariant<'a>,
}

enum OutputVariant<'a> {
    New {
        header: u64,
        name: String,
        already_exist: bool,
    },
    Msg {
        bytes: Cow<'a, [u8]>,
        bad_stream: bool,
    },
    Close {
        header: u64,
        error: Option<CloseError>,
    },
    Reset {
        header: u64,
    },
}

struct CloseError {
    stream_id: StreamId,
    kind: CloseErrorKind,
}

enum CloseErrorKind {
    OnlyOutgoing,
    OnlyIncoming,
    DoesntExist { incoming: bool },
}

impl fmt::Display for CloseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CloseErrorKind::OnlyOutgoing => write!(
                f,
                "closing incoming part of stream where only outgoing part remains"
            ),
            CloseErrorKind::OnlyIncoming => write!(
                f,
                "closing outgoing part of stream where only incoming part remains"
            ),
            CloseErrorKind::DoesntExist { incoming } => {
                let s = if *incoming { "incoming" } else { "outgoing" };
                write!(f, "closing {s} part of stream that doesn't exist")
            }
        }
    }
}

impl fmt::Display for CloseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let CloseError { stream_id, kind } = self;
        write!(f, "{stream_id}: {kind}")
    }
}

impl<Inner> State<Inner>
where
    Inner: HandleData + From<(u64, bool)>,
{
    // TODO: refactor it, implement `From<StreamId>` for `Inner`
    fn inner_state(stream_id: StreamId) -> Inner {
        match stream_id {
            StreamId::Handshake => panic!("mplex stream cannot be handshake"),
            StreamId::Forward(id) => Inner::from((id, true)),
            StreamId::Backward(id) => Inner::from((id, false)),
        }
    }

    fn process<'a>(&mut self, incoming: bool, bytes: &'a [u8]) -> Vec<Output<'a>> {
        let mut output = vec![];
        loop {
            let acc = if incoming {
                self.incoming.accumulate(bytes)
            } else {
                self.outgoing.accumulate(bytes)
            };
            if let Poll::Ready(o) = acc {
                let acc::Output {
                    tag,
                    stream_id,
                    bytes,
                } = o;
                match tag {
                    acc::Tag::New => {
                        let name = String::from_utf8(bytes.to_vec())
                            .unwrap_or_else(|_| hex::encode(bytes));
                        let stream = Self::inner_state(stream_id);
                        let already_exist = self
                            .inners
                            .insert(stream_id, Status::Duplex(stream))
                            .is_some();
                        let header = match stream_id {
                            StreamId::Handshake => u64::MAX,
                            StreamId::Forward(id) => id << 3,
                            StreamId::Backward(id) => id << 3,
                        };
                        let variant = OutputVariant::New {
                            header,
                            name,
                            already_exist,
                        };
                        output.push(Output { stream_id, variant })
                    }
                    acc::Tag::Msg => {
                        let bad_stream = !matches!(
                            (self.inners.get_mut(&stream_id), incoming),
                            (Some(Status::Duplex(_)), _)
                                | (Some(Status::IncomingOnly(_)), true)
                                | (Some(Status::OutgoingOnly(_)), false)
                        );
                        let variant = OutputVariant::Msg { bytes, bad_stream };
                        output.push(Output { stream_id, variant })
                    }
                    acc::Tag::Close => {
                        let error_kind = match (self.inners.remove(&stream_id), incoming) {
                            (Some(Status::Duplex(stream)), true) => {
                                self.inners.insert(stream_id, Status::OutgoingOnly(stream));
                                None
                            }
                            (Some(Status::Duplex(stream)), false) => {
                                self.inners.insert(stream_id, Status::IncomingOnly(stream));
                                None
                            }
                            (Some(Status::IncomingOnly(_)), true) => None,
                            (Some(Status::OutgoingOnly(_)), false) => None,
                            (Some(Status::OutgoingOnly(_)), true) => {
                                Some(CloseErrorKind::OnlyOutgoing)
                            }
                            (Some(Status::IncomingOnly(_)), false) => {
                                Some(CloseErrorKind::OnlyIncoming)
                            }
                            (None, incoming) => Some(CloseErrorKind::DoesntExist { incoming }),
                        };
                        let header = match stream_id {
                            StreamId::Handshake => u64::MAX,
                            StreamId::Forward(id) => 4 + (id << 3),
                            StreamId::Backward(id) => 3 + (id << 3),
                        };
                        let error = error_kind.map(|kind| CloseError { stream_id, kind });
                        let variant = OutputVariant::Close { header, error };
                        output.push(Output { stream_id, variant })
                    }
                    acc::Tag::Reset => {
                        self.inners.remove(&stream_id);
                        let header = match stream_id {
                            StreamId::Handshake => u64::MAX,
                            StreamId::Forward(id) => 6 + (id << 3),
                            StreamId::Backward(id) => 5 + (id << 3),
                        };
                        let variant = OutputVariant::Reset { header };
                        output.push(Output { stream_id, variant })
                    }
                };
            } else {
                break;
            }
        }

        output
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + From<(u64, bool)>,
{
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        for Output { stream_id, variant } in self.process(id.incoming, bytes) {
            let db_stream = self
                .stream
                .get_or_insert_with(|| db.add(stream_id, StreamKind::Mplex));

            match variant {
                OutputVariant::New {
                    header,
                    name,
                    already_exist,
                } => {
                    if already_exist {
                        log::warn!("{id}, {stream_id}: new stream \"{name}\", but already exist");
                    }
                    db_stream.add(&id, &header.to_be_bytes())?;
                }
                OutputVariant::Msg {
                    bytes,
                    bad_stream: true,
                } => {
                    let _ = bytes;
                    // most likely, this stream was recently reset,
                    // and peer still don't know about it
                    log::warn!("{id}, {stream_id}: message for stream that doesn't exist",);
                }
                OutputVariant::Msg {
                    mut bytes,
                    bad_stream: false,
                } => {
                    self.inners
                        .get_mut(&stream_id)
                        .expect("cannot fail, checked upper in stack")
                        .as_mut()
                        .on_data(id.clone(), bytes.to_mut(), cx, db)?;
                }
                OutputVariant::Close { header, error } => {
                    if let Some(error) = error {
                        log::error!("{id} {error}");
                    }
                    db_stream.add(&id, &header.to_be_bytes())?;
                }
                OutputVariant::Reset { header } => {
                    db_stream.add(&id, &header.to_be_bytes())?;
                }
            }
        }

        Ok(())
    }
}
