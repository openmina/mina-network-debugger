pub mod noise;
pub mod meshsub;
pub mod kademlia;
pub mod rpc;

use std::{fmt, str::FromStr, string::FromUtf8Error};

use serde::{Serialize, Deserialize};
use radiation::{Absorb, Emit};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("{_0}")]
    Serde(serde_json::Error),
    #[error("{_0}")]
    BinProt(binprot::Error),
    #[error("{_0}")]
    Protobuf(prost::DecodeError),
    #[error("{_0}")]
    Utf8(FromUtf8Error),
}

#[derive(Clone, Serialize, Deserialize, Absorb, Emit, PartialEq, Eq)]
#[tag(u16)]
pub enum MessageType {
    // meshsub
    #[tag(0x0100)]
    Subscribe,
    Unsubscribe,
    PublishExternalTransition,
    PublishSnarkPoolDiff,
    PublishTransactionPoolDiff,
    // kademlia
    #[tag(0x0200)]
    PutValue,
    GetValue,
    AddProvider,
    GetProviders,
    FindNode,
    Ping,
    // handshake
    HandshakePayload,
    // rpc
    Rpc {
        tag: String,
    },
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageType::Subscribe => write!(f, "subscribe"),
            MessageType::Unsubscribe => write!(f, "unsubscribe"),
            MessageType::PublishExternalTransition => write!(f, "publish_external_transition"),
            MessageType::PublishSnarkPoolDiff => write!(f, "publish_snark_pool_diff"),
            MessageType::PublishTransactionPoolDiff => write!(f, "publish_transaction_pool_diff"),
            MessageType::PutValue => write!(f, "put_value"),
            MessageType::GetValue => write!(f, "get_value"),
            MessageType::AddProvider => write!(f, "add_provider"),
            MessageType::GetProviders => write!(f, "get_providers"),
            MessageType::FindNode => write!(f, "find_node"),
            MessageType::Ping => write!(f, "ping"),
            MessageType::HandshakePayload => write!(f, "handshake_payload"),
            MessageType::Rpc { tag } => write!(f, "rpc_{tag}"),
        }
    }
}

impl FromStr for MessageType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "subscribe" => Ok(MessageType::Subscribe),
            "unsubscribe" => Ok(MessageType::Unsubscribe),
            "publish_external_transition" => Ok(MessageType::PublishExternalTransition),
            "publish_snark_pool_diff" => Ok(MessageType::PublishSnarkPoolDiff),
            "publish_transaction_pool_diff" => Ok(MessageType::PublishTransactionPoolDiff),
            "put_value" => Ok(MessageType::PutValue),
            "get_value" => Ok(MessageType::GetValue),
            "add_provider" => Ok(MessageType::AddProvider),
            "get_providers" => Ok(MessageType::GetProviders),
            "find_node" => Ok(MessageType::FindNode),
            "ping" => Ok(MessageType::Ping),
            "handshake_payload" => Ok(MessageType::HandshakePayload),
            _ => Err(()),
        }
    }
}
