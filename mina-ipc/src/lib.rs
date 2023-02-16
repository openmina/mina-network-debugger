#![forbid(unsafe_code)]

pub mod libp2p_ipc_capnp {
    include!(concat!(env!("OUT_DIR"), "/libp2p_ipc_capnp.rs"));
}

pub mod message;

pub type Error = capnp::Error;
pub type Result<T> = capnp::Result<T>;
