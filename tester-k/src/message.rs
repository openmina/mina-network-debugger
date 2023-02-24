use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    time::{SystemTime, Duration},
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
    pub timestamps: DbTestTimestampsReport,
    pub events: DbTestEventsReport,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DbTestTimestampsReport {
    pub start: SystemTime,
    pub end: SystemTime,
    pub group_report: Vec<DbTestTimeGroupReport>,
    pub total_messages: usize,
    pub ordered: bool,
    pub timestamps_filter_ok: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DbTestEventsReport {
    pub matching: bool,
    pub consistent: bool,
    pub events: Vec<DbEventWithMetadata>,
    pub debugger_events: Vec<DbEventWithMetadata>,
    pub network_events: BTreeMap<u32, Vec<BlockNetworkEvent>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlockNetworkEvent {
    pub producer_id: String,
    pub hash: String,
    pub block_height: u32,
    pub global_slot: u32,
    pub incoming: bool,
    pub time: SystemTime,
    pub better_time: SystemTime,
    pub latency: Option<Duration>,
    pub sender_addr: SocketAddr,
    pub receiver_addr: SocketAddr,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DbEventWithMetadata {
    pub time_microseconds: u64,
    pub events: Vec<DbEvent>,
}

impl DbEventWithMetadata {
    pub fn height(&self) -> u32 {
        match self.events.first() {
            Some(DbEvent::PublishGossip { msg: GossipNetMessageV2Short::TestMessage { height }, .. }) => *height,
            Some(DbEvent::ReceivedGossip { msg: GossipNetMessageV2Short::TestMessage { height }, .. }) => *height,
            None => u32::MAX,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum GossipNetMessageV2Short {
    TestMessage {
        height: u32,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum DbEvent {
    ReceivedGossip {
        peer_id: String,
        peer_address: String,
        msg: GossipNetMessageV2Short,
        hash: String,
    },
    PublishGossip {
        msg: GossipNetMessageV2Short,
        hash: String,
    },
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
