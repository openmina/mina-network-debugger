use std::{fmt, mem};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db};

pub struct State<Inner> {
    stream_id: u64,
    stream_forward: bool,
    incoming: Option<String>,
    outgoing: Option<String>,
    accumulator_incoming: Vec<u8>,
    accumulator_outgoing: Vec<u8>,
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
            inner: None,
        }
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

pub enum Output<Inner> {
    Nothing,
    Protocol(String),
    Inner(String, Inner),
}

impl<Inner> fmt::Display for Output<Inner>
where
    Inner: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Output::Nothing => Ok(()),
            Output::Protocol(p) => write!(f, "{p} suggested"),
            Output::Inner(p, inner) => write!(f, "{p} {inner}"),
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
            Output::Inner(p, mut inner) => {
                let inner_item = inner.next()?;
                *self = Output::Inner(p.clone(), inner);
                Some(Output::Inner(p, inner_item))
            }
        }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + DynamicProtocol,
    Inner::Output: IntoIterator,
{
    type Output = Output<<Inner::Output as IntoIterator>::IntoIter>;

    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> Self::Output {
        let (accumulator, done) = if id.incoming {
            (&mut self.accumulator_incoming, &mut self.incoming)
        } else {
            (&mut self.accumulator_outgoing, &mut self.outgoing)
        };
        if let Some(protocol) = done {
            let inner = self.inner.get_or_insert_with(|| {
                Inner::from_name(protocol, self.stream_id, self.stream_forward)
            });
            let inner_out = if accumulator.is_empty() {
                inner.on_data(id, bytes, cx, db)
            } else {
                let mut total = mem::take(accumulator);
                total.extend_from_slice(bytes);
                inner.on_data(id, &mut total, cx, db)
            };
            Output::Inner(protocol.clone(), inner_out.into_iter())
        } else {
            accumulator.extend_from_slice(bytes);
            let cursor = &mut accumulator.as_slice();
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
                    *done = Some(s.to_string());
                    break;
                } else {
                    log::error!("incoming: {} unparsed {}", id, hex::encode(msg));
                    *done = Some("ERROR".to_string());
                    break;
                }
            }
            *accumulator = (*cursor).to_vec();
            match done {
                None => Output::Nothing,
                Some(p) => Output::Protocol(p.clone()),
            }
        }
    }
}
