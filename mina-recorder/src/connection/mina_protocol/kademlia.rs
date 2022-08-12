use std::fmt;

use crate::database::{DbStream, StreamMeta, StreamKind};

use super::{HandleData, DirectedId, Cx, Db};

#[allow(clippy::derive_partial_eq_without_eq)]
mod pb {
    include!(concat!(env!("OUT_DIR"), "/kad.pb.rs"));
}

impl fmt::Display for pb::message::Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let addrs = self
            .addrs
            .iter()
            .map(|addr| {
                let mut acc = String::new();
                let mut input = addr.as_slice();
                while !input.is_empty() {
                    match multiaddr::Protocol::from_bytes(input) {
                        Ok((p, i)) => {
                            input = i;
                            acc = format!("{acc}{p}");
                        }
                        Err(err) => {
                            input = &[];
                            acc = format!("{acc}{err}");
                        }
                    }
                }
                acc
            })
            .collect::<Vec<_>>();
        write!(
            f,
            "Peer {{ id: {}, addrs: {:?}, connection: {:?} }}",
            hex::encode(&self.id),
            addrs,
            self.connection()
        )
    }
}

pub struct RawOutput(pb::Message);

impl fmt::Display for RawOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let print_peers =
            |peers: &[pb::message::Peer]| peers.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        f.debug_struct("Message")
            .field("type", &self.0.r#type())
            .field("cluster_level_raw", &self.0.cluster_level_raw)
            .field("key", &hex::encode(&self.0.key))
            .field("record", &self.0.record)
            .field("closer_peers", &print_peers(&self.0.closer_peers))
            .field("provider_peers", &print_peers(&self.0.provider_peers))
            .finish()
    }
}

#[derive(Default)]
pub struct State {
    stream: Option<DbStream>,
}

impl HandleData for State {
    type Output = RawOutput;

    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _: &mut Cx, db: &Db) -> Self::Output {
        use prost::{bytes::Bytes, Message};

        let stream = self.stream.get_or_insert_with(|| {
            // TODO:
            db.add(StreamMeta::Forward(0), StreamKind::Meshsub).unwrap()
        });
        stream.add(id.incoming, id.metadata.time, bytes).unwrap();

        let buf = Bytes::from(bytes.to_vec());
        let msg = <pb::Message as Message>::decode_length_delimited(buf).unwrap();
        RawOutput(msg)
    }
}
