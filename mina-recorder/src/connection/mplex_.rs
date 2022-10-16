use std::{collections::BTreeMap, borrow::Cow, task::Poll, fmt};

use crate::database::{DbStream, StreamId, StreamKind};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

#[derive(Default)]
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

                let stream_id = if v == 0 || initiator == INCOMING {
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
                            let acc = mem::take(&mut self.acc);

                            return Poll::Ready(Output {
                                tag,
                                stream_id,
                                bytes: Cow::Owned(acc[offset..].to_vec()),
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

#[derive(Debug, PartialEq, Eq)]
struct Output<'a> {
    stream_id: StreamId,
    variant: OutputVariant<'a>,
}

impl<'a> fmt::Display for Output<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Output { stream_id, variant } = self;
        write!(f, "{stream_id}::")?;
        match variant {
            OutputVariant::New {
                header,
                name,
                already_exist,
            } => {
                write!(f, "new({header}, {name:?}, {already_exist})")
            }
            OutputVariant::Msg { bytes, bad_stream } => {
                let bytes = hex::encode(bytes);
                write!(f, "msg({bytes}, {bad_stream})")
            }
            OutputVariant::Close {
                header,
                error: None,
            } => {
                write!(f, "close({header}, 0)")
            }
            OutputVariant::Close {
                header,
                error: Some(CloseError::OnlyOutgoing),
            } => {
                write!(f, "close({header}, 4)")
            }
            OutputVariant::Close {
                header,
                error: Some(CloseError::OnlyIncoming),
            } => {
                write!(f, "close({header}, 5)")
            }
            OutputVariant::Close {
                header,
                error: Some(CloseError::DoesntExist { incoming: true }),
            } => {
                write!(f, "close({header}, 6)")
            }
            OutputVariant::Close {
                header,
                error: Some(CloseError::DoesntExist { incoming: false }),
            } => {
                write!(f, "close({header}, 7)")
            }
            OutputVariant::Reset { header } => {
                write!(f, "reset({header})")
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
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

#[derive(Debug, PartialEq, Eq)]
enum CloseError {
    OnlyOutgoing,
    OnlyIncoming,
    DoesntExist { incoming: bool },
}

impl fmt::Display for CloseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CloseError::OnlyOutgoing => write!(
                f,
                "closing incoming part of stream where only outgoing part remains"
            ),
            CloseError::OnlyIncoming => write!(
                f,
                "closing outgoing part of stream where only incoming part remains"
            ),
            CloseError::DoesntExist { incoming } => {
                let s = if *incoming { "incoming" } else { "outgoing" };
                write!(f, "closing {s} part of stream that doesn't exist")
            }
        }
    }
}

impl<Inner> State<Inner>
where
    Inner: From<(u64, bool)>,
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
        let mut bytes = bytes;
        loop {
            let acc = if incoming {
                self.incoming.accumulate(bytes)
            } else {
                self.outgoing.accumulate(bytes)
            };
            bytes = &[];
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
                        let error = match (self.inners.remove(&stream_id), incoming) {
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
                            (Some(Status::OutgoingOnly(_)), true) => Some(CloseError::OnlyOutgoing),
                            (Some(Status::IncomingOnly(_)), false) => {
                                Some(CloseError::OnlyIncoming)
                            }
                            (None, incoming) => Some(CloseError::DoesntExist { incoming }),
                        };
                        let header = match stream_id {
                            StreamId::Handshake => u64::MAX,
                            StreamId::Forward(id) => 4 + (id << 3),
                            StreamId::Backward(id) => 3 + (id << 3),
                        };
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

#[cfg(test)]
mod tests {
    use super::State;

    fn generic(
        case: impl IntoIterator<Item = (bool, &'static str, impl IntoIterator<Item = &'static str>)>,
    ) {
        let mut state = State::<(u64, bool)>::default();
        for (incoming, bytes, expected) in case {
            let bytes = hex::decode(bytes).unwrap();
            for (actual, expected) in state.process(incoming, &bytes).into_iter().zip(expected) {
                assert_eq!(actual.to_string(), expected);
            }
        }
    }

    macro_rules! generic_test {
        ($name:ident, $case:expr) => {
            #[test]
            fn $name() {
                generic($case)
            }
        };
    }

    generic_test!(
        header_new,
        [(true, "0000", ["forward_00000000::new(0, \"\", false)"])]
    );
    generic_test!(
        header_new_1,
        [(true, "0800", ["forward_00000001::new(8, \"\", false)"])]
    );
    generic_test!(
        header_new_named,
        [(
            true,
            "1009736f6d655f6e616d65",
            ["forward_00000002::new(16, \"some_name\", false)"]
        ),]
    );
    generic_test!(
        header_new_repeat,
        [(
            true,
            "18001800",
            [
                "forward_00000003::new(24, \"\", false)",
                "forward_00000003::new(24, \"\", true)",
            ]
        )]
    );
    generic_test!(
        msg,
        [(
            true,
            "00000203abcdef",
            [
                "forward_00000000::new(0, \"\", false)",
                "forward_00000000::msg(abcdef, false)",
            ]
        )]
    );
    generic_test!(
        msg_opposite,
        [
            (true, "000002", ["forward_00000000::new(0, \"\", false)"]),
            (false, "0103", ["forward_00000000::msg(abcdef, false)"]),
        ]
    );
}
