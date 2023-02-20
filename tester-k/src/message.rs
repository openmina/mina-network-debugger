use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    time::SystemTime,
};

use mina_ipc::message::ChecksumPair;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
pub struct Summary {
    pub peer_id: String,
    pub node: Option<Report>,
    pub debugger: Option<DebuggerReport>,
    pub net_report: Vec<NetReport>,
}

#[derive(Serialize, Deserialize)]
pub struct Registered {
    pub info: PeerInfo,
    pub secret_key: String,
    pub external: IpAddr,
    pub peers: BTreeMap<IpAddr, String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PeerInfo {
    pub ip: IpAddr,
    pub peer_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Report {
    pub ipc: ChecksumPair,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DebuggerReport {
    pub version: String,
    pub ipc: ChecksumPair,
    pub network: BTreeMap<IpAddr, ConnectionMetadata>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct NetReport {
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub timestamp: SystemTime,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConnectionMetadata {
    pub incoming: bool,
    pub fd: i32,
    pub checksum: ChecksumPair,
    pub timestamp: SystemTime,
}
