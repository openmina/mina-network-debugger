pub mod meshsub;

use std::{fmt, mem};

use super::{DirectedId, HandleData, DynamicProtocol, Cx, logger};

pub enum State {
    Meshsub(meshsub::State),
    Rpc,
    Ipfs,
    Kad,
}

impl DynamicProtocol for State {
    fn from_name(name: &str) -> Self {
        match name {
            "/meshsub/1.1.0" => State::Meshsub(Default::default()),
            "coda/rpcs/0.0.1" => State::Rpc,
            "/ipfs/id/1.0.0" => State::Ipfs,
            "/coda/kad/1.0.0" => State::Kad,
            name => panic!("unknown protocol {name}"),
        }
    }
}

pub enum Output<Meshsub> {
    Nothing,
    Meshsub(Meshsub),
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
            Output::Other(inner) => Some(Output::Other(inner)),
        }
    }
}

impl HandleData for State {
    type Output = Output<<Vec<meshsub::Event> as IntoIterator>::IntoIter>;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        match self {
            State::Meshsub(inner) => Output::Meshsub(inner.on_data(id, bytes, cx).into_iter()),
            State::Rpc => Output::Other(().on_data(id, bytes, cx)),
            State::Ipfs => Output::Other(().on_data(id, bytes, cx)),
            State::Kad => Output::Other(().on_data(id, bytes, cx)),
        }
    }
}
