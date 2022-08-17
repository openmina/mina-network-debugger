pub mod meshsub;
pub mod kademlia;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("{_0}")]
    Serde(serde_json::Error),
    #[error("{_0}")]
    BinProt(bin_prot::error::Error),
    #[error("{_0}")]
    Protobuf(prost::DecodeError),
}
