use std::{fmt, mem};

use super::{DirectedId, HandleData, Cx};

#[derive(Default)]
pub struct State<Inner> {
    incoming_done: bool,
    outgoing_done: bool,
    accumulator_incoming: Vec<u8>,
    accumulator_outgoing: Vec<u8>,
    inner: Inner,
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

pub enum Output<Inner> {
    Nothing,
    Protocol(String),
    Inner(Inner),
}

impl<Inner> fmt::Display for Output<Inner>
where
    Inner: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Output::Nothing => Ok(()),
            Output::Protocol(p) => f.write_str(p),
            Output::Inner(inner) => inner.fmt(f),
        }
    }
}

impl<Inner> Iterator for Output<Inner>
where
    Inner: Iterator,
{
    type Item = Output<Inner::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        match mem::replace(self, Output::Nothing) {
            Output::Nothing => None,
            Output::Protocol(p) => Some(Output::Protocol(p)),
            Output::Inner(mut inner) => {
                let inner_item = inner.next()?;
                *self = Output::Inner(inner);
                Some(Output::Inner(inner_item))
            }
        }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData,
    Inner::Output: IntoIterator,
{
    type Output = Output<<Inner::Output as IntoIterator>::IntoIter>;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        let accumulator = if id.incoming {
            &mut self.accumulator_incoming
        } else {
            &mut self.accumulator_outgoing
        };
        if (id.incoming && !self.incoming_done) || (!id.incoming && !self.outgoing_done) {
            accumulator.extend_from_slice(bytes);
            let cursor = &mut accumulator.as_slice();
            let mut protocol = None;
            while let Some(msg) = take_msg(cursor) {
                if let Ok(s) = std::str::from_utf8(msg) {
                    let s = s.trim_end_matches('\n');
                    if s.starts_with("/multistream/") || s == "na" {
                        continue;
                    }
                    if s.starts_with("/libp2p/simultaneous-connect") {
                        // TODO: handle
                        continue;
                    }
                    protocol = Some(s.to_string());
                    if id.incoming {
                        self.incoming_done = true;
                    } else {
                        self.outgoing_done = true;
                    }
                    break;
                } else {
                    panic!(" .  unparsed message: {}", hex::encode(msg));
                }
            }
            *accumulator = (*cursor).to_vec();
            match protocol {
                None => Output::Nothing,
                Some(p) => Output::Protocol(p),
            }
        } else {
            let inner_out = if accumulator.is_empty() {
                self.inner.on_data(id, bytes, cx)
            } else {
                let mut total = mem::take(accumulator);
                total.extend_from_slice(bytes);
                self.inner.on_data(id, &mut total, cx)
            };
            Output::Inner(inner_out.into_iter())
        }
    }
}

#[cfg(test)]
#[test]
fn simple() {
    use crate::{DirectedId, EventMetadata};

    let bytes_1 = hex::decode("132f6d756c746973747265616d2f312e302e300a").unwrap();
    let bytes_2 = hex::decode("132f6d756c746973747265616d2f312e302e300a102f636f64612f6b61642f312e302e300a2c0804122600240801122059458f97a855040a767e890855941fb130dfa3fd5a9c8213bd73d716c2e697e15001").unwrap();
    let mut state = State::<()>::default();
    let mut id = DirectedId {
        metadata: EventMetadata::fake(),
        incoming: true,
    };
    for item in state.on_data(id.clone(), &mut bytes_2.clone(), &mut Cx::default()) {
        println!("{item}");
    }
    id.incoming = !id.incoming;
    for item in state.on_data(id.clone(), &mut bytes_1.clone(), &mut Cx::default()) {
        println!("{item}");
    }
}
