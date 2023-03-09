use std::{
    net::{IpAddr, SocketAddr},
    collections::BTreeMap,
    time::SystemTime,
};

use serde::{Serialize, Deserialize};

use mina_ipc::message::ChecksumPair;

use super::super::peer::tests::TestReport;

#[derive(Serialize, Deserialize)]
pub struct Registered {
    pub info: PeerInfo,
    pub secret_key: String,
    pub external: IpAddr,
    pub prev: Option<IpAddr>,
    pub peers: BTreeMap<IpAddr, String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PeerInfo {
    pub ip: IpAddr,
    pub peer_id: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Summary {
    pub peer_id: String,
    pub mock_report: Option<MockReport>,
    pub debugger_report: Option<DebuggerReport>,
    pub net_report: Vec<NetReport>,
}

impl Summary {
    pub fn full(&self) -> bool {
        self.mock_report.is_some() && self.debugger_report.is_some() && !self.net_report.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct NetReport {
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub counter: usize,
    pub timestamp: SystemTime,
    pub bytes_number: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MockReport {
    pub ipc: ChecksumPair,
    pub test: TestReport,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DebuggerReport {
    pub version: String,
    pub ipc: ChecksumPair,
    pub network: Vec<ConnectionMetadata>,
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
