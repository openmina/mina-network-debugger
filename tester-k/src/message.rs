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
    pub db_test: DbTestReport,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DbTestReport {
    pub start: SystemTime,
    pub end: SystemTime,
    pub group_report: Vec<DbTestTimeGroupReport>,
    pub total_messages: usize,
    pub ordered: bool,
    pub timestamps_filter_ok: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DbTestTimeGroupReport {
    pub timestamps: Vec<SystemTime>,
    pub timestamps_filter_ok: bool,
    pub ordered: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DebuggerReport {
    pub version: String,
    pub ipc: ChecksumPair,
    pub network: Vec<ConnectionMetadata>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct NetReport {
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub counter: usize,
    pub timestamp: SystemTime,
    pub bytes_number: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ConnectionMetadata {
    pub ip: IpAddr,
    pub counter: usize,
    pub incoming: bool,
    pub fd: i32,
    pub checksum: ChecksumPair,
    pub timestamp: SystemTime,
}
