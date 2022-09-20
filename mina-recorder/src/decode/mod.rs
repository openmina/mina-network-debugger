pub mod noise;
pub mod meshsub;
pub mod kademlia;
pub mod rpc;
pub mod identify;
pub mod json_string;

mod utils;

use std::{fmt, str::FromStr, string::FromUtf8Error};

use serde::{Serialize, Deserialize};
use radiation::{Absorb, Emit};

use thiserror::Error;

use mina_p2p_messages::rpc::JSONinifyError;

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
    #[error("wrong size: {actual} != {expected}")]
    UnexpectedSize { actual: usize, expected: usize },
}

impl From<binprot::Error> for DecodeError {
    fn from(v: binprot::Error) -> Self {
        DecodeError::BinProt(v)
    }
}

impl From<JSONinifyError> for DecodeError {
    fn from(v: JSONinifyError) -> Self {
        match v {
            JSONinifyError::Binprot(err) => DecodeError::BinProt(err),
            JSONinifyError::JSON(err) => DecodeError::Serde(err),
        }
    }
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
    Control,
    // kademlia
    #[tag(0x0200)]
    PutValue,
    GetValue,
    AddProvider,
    GetProviders,
    FindNode,
    Ping,
    // handshake
    #[tag(0x0300)]
    HandshakePayload,
    // rpc
    #[tag(0x0400)]
    RpcMenu,
    GetSomeInitialPeers,
    GetStagedLedgerAuxAndPendingCoinbasesAtHash,
    AnswerSyncLedgerQuery,
    GetAncestry,
    GetBestTip,
    GetNodeStatus,
    GetTransitionChainProof,
    GetTransitionChain,
    GetTransitionKnowledge,
    GetEpochLedger,
    BanNotify,
    // identify
    #[tag(0x0500)]
    Identify,
    IdentifyPush,
    // peer exchange
    #[tag(0x0600)]
    PeerExchange,
    #[tag(0x0700)]
    BitswapExchange,
    #[tag(0x0800)]
    NodeStatus,
    #[tag(0x0900)]
    Select,
    #[tag(0x0a00)]
    Mplex,
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageType::Subscribe => write!(f, "subscribe"),
            MessageType::Unsubscribe => write!(f, "unsubscribe"),
            MessageType::PublishExternalTransition => write!(f, "publish_external_transition"),
            MessageType::PublishSnarkPoolDiff => write!(f, "publish_snark_pool_diff"),
            MessageType::PublishTransactionPoolDiff => write!(f, "publish_transaction_pool_diff"),
            MessageType::Control => write!(f, "meshsub_control"),
            MessageType::PutValue => write!(f, "put_value"),
            MessageType::GetValue => write!(f, "get_value"),
            MessageType::AddProvider => write!(f, "add_provider"),
            MessageType::GetProviders => write!(f, "get_providers"),
            MessageType::FindNode => write!(f, "find_node"),
            MessageType::Ping => write!(f, "ping"),
            MessageType::HandshakePayload => write!(f, "handshake_payload"),
            MessageType::RpcMenu => write!(f, "__Versioned_rpc.Menu"),
            MessageType::GetSomeInitialPeers => write!(f, "get_some_initial_peers"),
            MessageType::GetStagedLedgerAuxAndPendingCoinbasesAtHash => {
                write!(f, "get_staged_ledger_aux_and_pending_coinbases_at_hash")
            }
            MessageType::AnswerSyncLedgerQuery => write!(f, "answer_sync_ledger_query"),
            MessageType::GetAncestry => write!(f, "get_ancestry"),
            MessageType::GetBestTip => write!(f, "get_best_tip"),
            MessageType::GetNodeStatus => write!(f, "get_node_status"),
            MessageType::GetTransitionChainProof => write!(f, "get_transition_chain_proof"),
            MessageType::GetTransitionChain => write!(f, "get_transition_chain"),
            MessageType::GetTransitionKnowledge => write!(f, "get_transition_knowledge"),
            MessageType::GetEpochLedger => write!(f, "get_epoch_ledger"),
            MessageType::BanNotify => write!(f, "ban_notify"),
            MessageType::Identify => write!(f, "identify"),
            MessageType::IdentifyPush => write!(f, "identify_push"),
            MessageType::PeerExchange => write!(f, "peer_exchange"),
            MessageType::BitswapExchange => write!(f, "bitswap_exchange"),
            MessageType::NodeStatus => write!(f, "node_status"),
            MessageType::Select => write!(f, "select"),
            MessageType::Mplex => write!(f, "mplex"),
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
            "meshsub_control" => Ok(MessageType::Control),
            "put_value" => Ok(MessageType::PutValue),
            "get_value" => Ok(MessageType::GetValue),
            "add_provider" => Ok(MessageType::AddProvider),
            "get_providers" => Ok(MessageType::GetProviders),
            "find_node" => Ok(MessageType::FindNode),
            "ping" => Ok(MessageType::Ping),
            "handshake_payload" => Ok(MessageType::HandshakePayload),
            "__Versioned_rpc.Menu" => Ok(MessageType::RpcMenu),
            "get_some_initial_peers" => Ok(MessageType::GetSomeInitialPeers),
            "get_staged_ledger_aux_and_pending_coinbases_at_hash" => {
                Ok(MessageType::GetStagedLedgerAuxAndPendingCoinbasesAtHash)
            }
            "answer_sync_ledger_query" => Ok(MessageType::AnswerSyncLedgerQuery),
            "get_ancestry" => Ok(MessageType::GetAncestry),
            "get_best_tip" => Ok(MessageType::GetBestTip),
            "get_node_status" => Ok(MessageType::GetNodeStatus),
            "get_transition_chain_proof" => Ok(MessageType::GetTransitionChainProof),
            "get_transition_chain" => Ok(MessageType::GetTransitionChain),
            "get_transition_knowledge" => Ok(MessageType::GetTransitionKnowledge),
            "get_epoch_ledger" => Ok(MessageType::GetEpochLedger),
            "ban_notify" => Ok(MessageType::BanNotify),
            "identify" => Ok(MessageType::Identify),
            "identify_push" => Ok(MessageType::IdentifyPush),
            "peer_exchange" => Ok(MessageType::PeerExchange),
            "bitswap_exchange" => Ok(MessageType::BitswapExchange),
            "node_status" => Ok(MessageType::NodeStatus),
            "select" => Ok(MessageType::Select),
            "mplex" => Ok(MessageType::Mplex),
            _ => Err(()),
        }
    }
}
