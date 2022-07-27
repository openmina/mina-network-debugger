use std::{fmt, mem};

use super::{DirectedId, HandleData, Cx};

#[derive(Default)]
pub struct State<Inner> {
    incoming_done: bool,
    outgoing_done: bool,
    accumulator: Vec<u8>,
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
    Accumulating,
    Protocol(String),
    Inner(Inner),
}

impl<Inner> fmt::Display for Output<Inner>
where
    Inner: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Output::Accumulating => Ok(()),
            Output::Protocol(p) => f.write_str(p),
            Output::Inner(inner) => write!(f, "{inner}"),
        }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData,
{
    type Output = Output<Inner::Output>;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        if (id.incoming && !self.incoming_done) || (!id.incoming && !self.outgoing_done) {
            self.accumulator.extend_from_slice(bytes);
            let cursor = &mut self.accumulator.as_slice();
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
                    log::error!(" .  unparsed message: {}", hex::encode(msg));
                }
            }
            self.accumulator = (*cursor).to_vec();
            match protocol {
                None => Output::Accumulating,
                Some(p) => Output::Protocol(p),
            }
        } else {
            let inner_out = if self.accumulator.is_empty() {
                self.inner.on_data(id, bytes, cx)
            } else {
                let mut total = mem::take(&mut self.accumulator);
                total.extend_from_slice(bytes);
                self.inner.on_data(id, &mut total, cx)
            };
            Output::Inner(inner_out)
        }
    }
}
