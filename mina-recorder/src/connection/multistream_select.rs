use std::{mem, str};

use crate::database::{DbStream, StreamId, StreamKind};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

pub struct State<Inner> {
    stream_id: u64,
    stream_forward: bool,
    incoming: Option<String>,
    outgoing: Option<String>,
    accumulator_incoming: Vec<u8>,
    accumulator_outgoing: Vec<u8>,
    simultaneous_connect_incoming: bool,
    simultaneous_connect_outgoing: bool,
    error: bool,
    stream: Option<DbStream>,
    inner: Option<Inner>,
}

impl<Inner> From<(u64, bool)> for State<Inner> {
    fn from((stream_id, stream_forward): (u64, bool)) -> Self {
        State {
            stream_id,
            stream_forward,
            incoming: None,
            outgoing: None,
            accumulator_incoming: vec![],
            accumulator_outgoing: vec![],
            simultaneous_connect_incoming: false,
            simultaneous_connect_outgoing: false,
            error: false,
            stream: None,
            inner: None,
        }
    }
}

fn parse_sc(cursor: &mut &[u8]) -> bool {
    if (*cursor).starts_with(b"\ninitiator\n") | (*cursor).starts_with(b"\nresponder\n") {
        *cursor = &(*cursor)[11..];
        true
    } else {
        false
    }
}

fn take_msg<'a>(cursor: &mut &'a [u8]) -> Option<&'a [u8]> {
    // TODO: unsigned variable-length integer
    // https://github.com/multiformats/unsigned-varint
    let length = *(*cursor).first()? as usize;
    if cursor.len() < length + 1 {
        return None;
    }
    *cursor = &cursor[1..];

    let (msg, remaining) = cursor.split_at(length);
    *cursor = remaining;
    Some(msg)
}

#[derive(Debug)]
enum MultistreamResult {
    Message(String),
    MessageWithAccumulatedData(String, Vec<u8>),
    Done(String),
    Error(Vec<u8>),
    Pending,
}

impl<Inner> State<Inner> {
    fn on_data_inner(&mut self, incoming: bool, bytes: &[u8]) -> MultistreamResult {
        let (accumulator, done, other, sc, sc_other) = if incoming {
            (
                &mut self.accumulator_incoming,
                &mut self.incoming,
                &mut self.outgoing,
                &mut self.simultaneous_connect_incoming,
                &self.simultaneous_connect_outgoing,
            )
        } else {
            (
                &mut self.accumulator_outgoing,
                &mut self.outgoing,
                &mut self.incoming,
                &mut self.simultaneous_connect_outgoing,
                &self.simultaneous_connect_incoming,
            )
        };
        if let Some(protocol) = done {
            if accumulator.is_empty() {
                MultistreamResult::Message(protocol.clone())
            } else {
                let mut total = mem::take(accumulator);
                total.extend_from_slice(bytes);
                MultistreamResult::MessageWithAccumulatedData(protocol.clone(), total)
            }
        } else {
            accumulator.extend_from_slice(bytes);
            let cursor = &mut accumulator.as_slice();
            loop {
                if parse_sc(cursor) {
                    *sc = false;
                }
                let msg = match take_msg(cursor) {
                    Some(v) => v,
                    None => break,
                };
                if let Ok(s) = str::from_utf8(msg) {
                    let s = s.trim_end_matches('\n');
                    if s.starts_with("/multistream/") || s == "na" {
                        continue;
                    }
                    if s.starts_with("/libp2p/simultaneous-connect") {
                        *sc = true;
                        if *sc_other {
                            // both parties initiating simultaneously
                            *other = None;
                        }
                        continue;
                    }
                    if s.starts_with("select") {
                        continue;
                    }
                    let s = s.to_string();
                    *accumulator = (*cursor).to_vec();
                    if !(*sc && *sc_other) {
                        *done = Some(s.clone());
                        return MultistreamResult::Done(s);
                    }
                    break;
                } else {
                    return MultistreamResult::Error(msg.to_vec());
                }
            }
            MultistreamResult::Pending
        }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + DynamicProtocol,
{
    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        if self.error {
            return Ok(());
        }

        match self.on_data_inner(id.incoming, bytes) {
            MultistreamResult::Message(protocol) => {
                let inner = self.inner.get_or_insert_with(|| {
                    Inner::from_name(&protocol, self.stream_id, self.stream_forward)
                });
                inner.on_data(id, bytes, cx, db)
            }
            MultistreamResult::MessageWithAccumulatedData(protocol, mut data) => {
                let inner = self.inner.get_or_insert_with(|| {
                    Inner::from_name(&protocol, self.stream_id, self.stream_forward)
                });
                inner.on_data(id, &mut data, cx, db)
            }
            MultistreamResult::Done(protocol) => {
                let stream = self.stream.get_or_insert_with(|| {
                    let stream_id = if self.stream_forward {
                        StreamId::Forward(self.stream_id)
                    } else {
                        StreamId::Backward(self.stream_id)
                    };
                    db.add(stream_id, StreamKind::Select)
                });
                stream.add(id.incoming, id.metadata.time, protocol.as_bytes())
            }
            MultistreamResult::Error(msg) => {
                log::error!(
                    "{id}, {}, stream_id: {}, unparsed {}",
                    db.id(),
                    self.stream_id,
                    hex::encode(msg)
                );
                self.error = true;
                Ok(())
            }
            MultistreamResult::Pending => Ok(()),
        }
    }
}

#[cfg(test)]
#[test]
#[rustfmt::skip]
fn simultaneous_connect_test() {
    let mut state = State::<()>::from((0, false));

    let mut data = hex::decode("132f6d756c746973747265616d2f312e302e300a1d2f6c69627032702f73696d756c74616e656f75732d636f6e6e6563740a072f6e6f6973650a").expect("valid constant");
    let result = state.on_data_inner(false, &mut data);
    assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));

    let mut data = hex::decode("132f6d756c746973747265616d2f312e302e300a1d2f6c69627032702f73696d756c74616e656f75732d636f6e6e6563740a072f6e6f6973650a1c73656c6563743a31383333363733363237323438313935323033380a").expect("valid constant");
    let result = state.on_data_inner(true, &mut data);
    assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));

    let mut data = hex::decode("1c73656c6563743a31343838333538303531393436383433383239370a0a726573706f6e6465720a").expect("valid constant");
    let result = state.on_data_inner(false, &mut data);
    assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));

    let mut data = hex::decode("0a696e69746961746f720a072f6e6f6973650a").expect("valid constant");
    let result = state.on_data_inner(true, &mut data);
    assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));

    let mut data = hex::decode("072f6e6f6973650a").expect("valid constant");
    let result = state.on_data_inner(false, &mut data);
    assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));

    let mut data = hex::decode("0020c29c4aa9bc861ac3163bfc562ab3f1ca984440f50ca7944ab1fcb40b398bac34").expect("valid constant");
    let result = state.on_data_inner(true, &mut data);
    assert!(matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));
}

#[cfg(test)]
#[test]
#[rustfmt::skip]
fn simultaneous_connect_with_accumulator_test() {
    let mut state = State::<()>::from((0, false));

    let mut data = hex::decode("132f6d756c746973747265616d2f312e302e300a1d2f6c69627032702f73696d756c74616e656f75732d636f6e6e6563740a072f6e6f6973650a").expect("valid constant");
    let result = state.on_data_inner(false, &mut data);
    assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));

    let mut data = hex::decode("132f6d756c746973747265616d2f312e302e300a1d2f6c69627032702f73696d756c74616e656f75732d636f6e6e6563740a072f6e6f6973650a1c73656c6563743a31383333363733363237323438313935323033380a").expect("valid constant");
    let chunks = [1, 19, 1, 29, 1, 7, 1, 28];
    for chunk in chunks {
        let mut chunk_data = data.drain(..chunk).collect::<Vec<u8>>();
        let result = state.on_data_inner(true, &mut chunk_data);
        assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));
    }

    let mut data = hex::decode("1c73656c6563743a31343838333538303531393436383433383239370a0a726573706f6e6465720a").expect("valid constant");
    let chunks = [29, 11];
    for chunk in chunks {
        let mut chunk_data = data.drain(..chunk).collect::<Vec<u8>>();
        let result = state.on_data_inner(false, &mut chunk_data);
        assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));
    }

    let mut data = hex::decode("0a696e69746961746f720a072f6e6f6973650a").expect("valid constant");
    let chunks = [1, 10, 1, 7];
    for chunk in chunks {
        let mut chunk_data = data.drain(..chunk).collect::<Vec<u8>>();
        let result = state.on_data_inner(true, &mut chunk_data);
        assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));
    }

    let mut data = hex::decode("072f6e6f6973650a").expect("valid constant");
    let result = state.on_data_inner(false, &mut data);
    assert!(!matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));

    let mut data = hex::decode("0020c29c4aa9bc861ac3163bfc562ab3f1ca984440f50ca7944ab1fcb40b398bac34").expect("valid constant");
    let result = state.on_data_inner(true, &mut data);
    assert!(matches!(result, MultistreamResult::Message(_) | MultistreamResult::MessageWithAccumulatedData(..)));
}
