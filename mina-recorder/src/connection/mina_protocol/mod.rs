pub mod meshsub;
pub mod kademlia;

use std::{fmt, mem};

use crate::database::{StreamMeta, StreamKind, DbStream};

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db};

pub struct State {
    meta: StreamMeta,
    kind: StreamKind,
    stream: Option<DbStream>,
}

impl DynamicProtocol for State {
    fn from_name(name: &str, id: u64, forward: bool) -> Self {
        let meta = if forward {
            StreamMeta::Forward(id)
        } else {
            StreamMeta::Backward(id)
        };
        let kind = match name {
            "/meshsub/1.1.0" => StreamKind::Meshsub,
            "coda/rpcs/0.0.1" => StreamKind::Rpc,
            "/ipfs/id/1.0.0" => StreamKind::IpfsId,
            "/coda/kad/1.0.0" => StreamKind::Kad,
            "/mina/peer-exchange" => StreamKind::PeerExchange,
            name => panic!("unknown protocol {name}"),
        };
        State {
            meta,
            kind,
            stream: None,
        }
    }
}

pub enum Output {
    Nothing,
}

impl fmt::Display for Output {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Output::Nothing => Ok(()),
        }
    }
}

impl Iterator for Output {
    type Item = Output;

    fn next(&mut self) -> Option<Self::Item> {
        match mem::replace(self, Output::Nothing) {
            Output::Nothing => None,
        }
    }
}

impl HandleData for State {
    type Output = Output;

    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], _: &mut Cx, db: &Db) -> Self::Output {
        let stream = self
            .stream
            .get_or_insert_with(|| db.add(self.meta, self.kind).unwrap());
        stream.add(id.incoming, id.metadata.time, bytes).unwrap();
        Output::Nothing
    }
}
