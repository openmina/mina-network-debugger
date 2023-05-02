/// Contains header for kernel events.
mod event;
pub use self::event::{EventMetadata, ConnectionInfo, DirectedId};

/// Represents chunk of raw data flown in TCP connection.
mod chunk;
pub use self::chunk::{ChunkHeader, EncryptionStatus, ChunkParser};

/// State machine that manages debuggee processes and their TCP connections.
mod recorder;
pub use self::recorder::P2pRecorder;

/// State machine that manages snark worker processes.
mod snark_worker;
pub use self::snark_worker::*;

pub mod tester;

/// State machine that manages the state of one TCP connection.
mod connection;
pub use self::connection::yamux;

/// Data is stored on persistent storage in the same encoding as it going on wire.
/// This module contains decoders that transform binary data to JSON.
mod decode;
pub use self::decode::{meshsub, meshsub_stats};

/// Helps encode/decode data for database.
pub mod custom_coding;

/// Everything related to rocksdb.
pub mod database;

/// HTTP or HTTPS server. The interface to the whole debugger.
pub mod server;

/// Obsolete. Attempt to store all strace log in database.
pub mod strace;

/// Obsolete. Pausing the node if ring buffer between kernel and userspace is about to overflow.
pub mod ptrace;

/// Observers blocks, snarks and transactions and store metadata about it.
/// Especially, it determines block latency in the node.
mod stats;

/// Tests for `stats` module.
#[cfg(test)]
mod stats_test;

/// Decodes capnp encoded IPC between mina deamon and libp2p_helper.
pub mod libp2p_helper;

/// Generated code that facilitate capnp decoding.
pub mod libp2p_ipc_capnp {
    include!(concat!(env!("OUT_DIR"), "/libp2p_ipc_capnp.rs"));
}

pub mod application;
