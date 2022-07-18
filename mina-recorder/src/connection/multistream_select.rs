use super::{ConnectionId, HandleData};

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

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData,
{
    fn on_data(&mut self, id: ConnectionId, incoming: bool, bytes: Vec<u8>) {
        // log::info!("{addr} {fd} {arrow} {alias} \"{}\"", hex::encode(bytes));

        // WARNING: hack to see when noise handshake begins
        let noise_prefix = bytes.starts_with(b"\x00");
        if ((incoming && !self.incoming_done) || (!incoming && !self.outgoing_done))
            && !noise_prefix
        {
            let ConnectionId { alias, addr, fd } = id;
            let arrow = if incoming { "->" } else { "<-" };

            self.accumulator.extend_from_slice(&bytes);
            let cursor = &mut self.accumulator.as_slice();
            while let Some(msg) = take_msg(cursor) {
                if let Ok(s) = std::str::from_utf8(msg) {
                    let s = s.trim_end_matches('\n');
                    log::info!("{addr} {fd} {arrow} {alias} \"{s}\"");
                } else {
                    log::error!(" .  unparsed message: {}", hex::encode(msg));
                }
            }
            self.accumulator = (*cursor).to_vec();
        } else {
            if noise_prefix {
                if incoming {
                    self.incoming_done = true;
                } else {
                    self.outgoing_done = true;
                }
            }
            self.inner.on_data(id, incoming, bytes);
        }
    }
}
