pub mod meshsub;
pub mod kademlia;

use std::{fmt, mem};

use super::{DirectedId, HandleData, DynamicProtocol, Cx, logger};

pub enum State {
    Meshsub(meshsub::State),
    Rpc,
    Ipfs,
    Kad(kademlia::State),
    PeerExchange,
}

impl DynamicProtocol for State {
    fn from_name(name: &str) -> Self {
        match name {
            "/meshsub/1.1.0" => State::Meshsub(Default::default()),
            "coda/rpcs/0.0.1" => State::Rpc,
            "/ipfs/id/1.0.0" => State::Ipfs,
            "/coda/kad/1.0.0" => State::Kad(Default::default()),
            "/mina/peer-exchange" => State::PeerExchange,
            name => panic!("unknown protocol {name}"),
        }
    }
}

pub enum Output<Meshsub> {
    Nothing,
    Meshsub(Meshsub),
    Kad(String),
    Other(logger::Output),
}

impl<Meshsub> fmt::Display for Output<Meshsub>
where
    Meshsub: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Output::Nothing => Ok(()),
            Output::Meshsub(inner) => inner.fmt(f),
            Output::Kad(inner) => inner.fmt(f),
            Output::Other(inner) => inner.fmt(f),
        }
    }
}

impl<Meshsub> Iterator for Output<Meshsub>
where
    Meshsub: Iterator,
{
    type Item = Output<Meshsub::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        match mem::replace(self, Output::Nothing) {
            Output::Nothing => None,
            Output::Meshsub(mut inner) => {
                let inner_item = inner.next()?;
                *self = Output::Meshsub(inner);
                Some(Output::Meshsub(inner_item))
            }
            Output::Kad(inner) => Some(Output::Kad(inner)),
            Output::Other(inner) => Some(Output::Other(inner)),
        }
    }
}

impl HandleData for State {
    type Output = Output<<Vec<meshsub::Event> as IntoIterator>::IntoIter>;

    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        match self {
            State::Meshsub(inner) => Output::Meshsub(inner.on_data(id, bytes, cx).into_iter()),
            State::Rpc => Output::Other(().on_data(id, bytes, cx)),
            State::Ipfs => Output::Other(().on_data(id, bytes, cx)),
            State::Kad(inner) => Output::Kad(inner.on_data(id, bytes, cx)),
            State::PeerExchange => Output::Other(().on_data(id, bytes, cx)),
        }
    }
}
