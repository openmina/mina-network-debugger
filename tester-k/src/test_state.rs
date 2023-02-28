use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

use mina_ipc::message::ChecksumPair;

use crate::{
    libp2p_helper::Process,
    message::{NetReport, PeerInfo, Registered, Report, DebuggerReport, Summary, DbTestReport},
};

pub struct State {
    process: Process,
    build_number: u32,
    pub summary: BTreeMap<IpAddr, Summary>,
    pub test_result: Option<TestResult>,
}

#[derive(Serialize, Deserialize)]
pub struct TestResult {
    success: bool,
    ipc_ok: bool,
    connections_ok: bool,
    connections_order_ok: bool,
    db_order_ok: bool,
    db_events_ok: bool,
    db_events_consistent_ok: bool,
    debugger_version: Option<String>,
    ipc_verbose: Verbose,
    network_verbose: BTreeMap<IpAddr, NetworkVerbose>,
    db_tests: BTreeMap<IpAddr, DbTestReport>,
}

#[derive(Default, Serialize, Deserialize)]
struct Verbose {
    matches: Vec<IpcTestResult>,
    mismatches: Vec<IpcTestResult>,
}

#[derive(Serialize, Deserialize, Clone)]
struct IpcTestResult {
    ip: IpAddr,
    node_crc64: ChecksumPair,
    debugger_crc64: ChecksumPair,
}

#[derive(Default, Serialize, Deserialize)]
struct NetworkVerbose {
    matches: Vec<NetworkMatches>,
    checksum_mismatch: Vec<NetworkMatches>,
    local_debugger_missing: Vec<NetworkMatches>,
    remote_debugger_missing: Vec<NetworkMatches>,
    both_debuggers_missing: Vec<NetworkMatches>,
}

#[derive(Serialize, Deserialize)]
struct NetworkMatches {
    bytes_number: u64,
    timestamp: SystemTime,
    local: SocketAddr,
    local_time: Option<SystemTime>,
    local_crc64: Option<ChecksumPair>,
    remote: SocketAddr,
    remote_time: Option<SystemTime>,
    remote_crc64: Option<ChecksumPair>,
}

impl State {
    const SEED_NODES: usize = 10;

    pub fn new() -> Self {
        let (process, _) = Process::spawn();
        State {
            process,
            build_number: 0,
            summary: BTreeMap::default(),
            test_result: None,
        }
    }

    pub fn build_number(&self) -> u32 {
        self.build_number
    }

    pub fn register(&mut self, addr: SocketAddr, build_number: u32) -> anyhow::Result<Registered> {
        if self.build_number != build_number {
            log::info!(
                "current build: {}, this build: {}, cleanup",
                self.build_number,
                build_number,
            );
            self.build_number = build_number;
            self.summary.clear();
            self.test_result = None;
        }
        let Some((peer_id, _, secret_key)) = self.process.generate_keypair()? else {
            return Err(anyhow::anyhow!("cannot generate key pair"));
        };

        log::debug!(
            "already registered {} nodes, new peer_id {peer_id}",
            self.summary.len()
        );

        let info = PeerInfo {
            ip: addr.ip(),
            peer_id: peer_id.clone(),
        };
        let response = Registered {
            info: info.clone(),
            secret_key: hex::encode(secret_key),
            external: addr.ip(),
            peers: self
                .summary
                .iter()
                .take(Self::SEED_NODES)
                .map(|(k, s)| (*k, s.peer_id.clone()))
                .collect(),
        };
        self.summary.insert(
            addr.ip(),
            Summary {
                peer_id,
                ..Default::default()
            },
        );

        Ok(response)
    }

    pub fn reset(&mut self) {
        self.summary.clear();
        self.test_result = None;
    }

    pub fn add_node_report(&mut self, addr: SocketAddr, report: Report) {
        if let Some(summary) = self.summary.get_mut(&addr.ip()) {
            summary.node = Some(report);
        }
        self.perform_test();
    }

    pub fn add_debugger_report(&mut self, addr: SocketAddr, report: DebuggerReport) {
        if let Some(summary) = self.summary.get_mut(&addr.ip()) {
            summary.debugger = Some(report);
        }
        self.perform_test();
    }

    pub fn add_net_report(&mut self, addr: SocketAddr, report: Vec<NetReport>) {
        if let Some(summary) = self.summary.get_mut(&addr.ip()) {
            summary.net_report = report;
        }
        self.perform_test();
    }

    pub fn perform_test(&mut self) {
        let mut result = TestResult {
            success: true,
            ipc_ok: true,
            connections_order_ok: true,
            connections_ok: true,
            db_order_ok: true,
            db_events_ok: true,
            db_events_consistent_ok: true,
            debugger_version: None,
            ipc_verbose: Verbose::default(),
            network_verbose: BTreeMap::default(),
            db_tests: BTreeMap::default(),
        };
        if !self
            .summary
            .values()
            .all(|x| x.node.is_some() && x.debugger.is_some() && !x.net_report.is_empty())
        {
            return;
        }

        log::info!("collected all reports, checking...");
        for (&ip, summary) in &self.summary {
            let s_debugger = summary
                .debugger
                .as_ref()
                .expect("cannot fail, checked above");
            let s_node = summary.node.as_ref().expect("cannot fail, checked above");

            if result.debugger_version.is_none() {
                result.debugger_version = Some(s_debugger.version.clone());
                log::info!("debugger version: {}", s_debugger.version);
            }

            let ipc_test_result = IpcTestResult {
                ip,
                node_crc64: s_node.ipc.clone(),
                debugger_crc64: s_debugger.ipc.clone(),
            };
            if s_node.ipc.matches_(&s_debugger.ipc) {
                result.ipc_verbose.matches.push(ipc_test_result);
            } else {
                log::error!("test failed, ipc checksum mismatch at {ip}");
                result.success = false;
                result.ipc_ok = false;
                result.ipc_verbose.mismatches.push(ipc_test_result);
            }

            result.db_tests.insert(ip, s_node.db_test.clone());
            result.db_order_ok &= s_node.db_test.timestamps.total_messages > 0 && s_node.db_test.timestamps.ordered && s_node.db_test.timestamps.timestamps_filter_ok;
            if !result.db_order_ok {
                result.success = false;
            }

            result.db_events_ok &= !s_node.db_test.events.events.is_empty() && s_node.db_test.events.matching;
            if !result.db_events_ok {
                result.success = false;
            }

            result.db_events_consistent_ok &= s_node.db_test.events.consistent;
            if !result.db_events_consistent_ok {
                result.success = false;
            }

            // TODO: disable this test, it might be microseconds difference due to libp2p threads
            // let mut temp = s_debugger.network.clone();
            // temp.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            // if temp != s_debugger.network {
            //     result.success = false;
            //     result.connections_order_ok = false;
            //     log::error!("connections unordered at {ip}");
            // }

            // for each connection seen by tcpflow
            // must exist only one debugger who seen this connection as incoming
            // must exist only one (distinct) debugger who seen this connection as outgoing
            let mut network_verbose = NetworkVerbose::default();
            for r in &summary.net_report {
                let NetReport {
                    local,
                    remote,
                    counter,
                    timestamp,
                    bytes_number,
                } = *r;

                let remote_cn = self
                    .summary
                    .get(&remote.ip())
                    .and_then(|s| s.debugger.as_ref())
                    .and_then(|dbg| dbg.network.iter().find(|cn| cn.ip == local.ip() && cn.counter == counter));
                let local_cn = s_debugger.network.iter().find(|cn| cn.ip == remote.ip() && cn.counter == counter);

                match (local_cn, remote_cn) {
                    (Some(l), Some(r)) => {
                        let item = NetworkMatches {
                            bytes_number,
                            timestamp,
                            local,
                            local_time: Some(l.timestamp),
                            local_crc64: Some(l.checksum.clone()),
                            remote,
                            remote_time: Some(r.timestamp),
                            remote_crc64: Some(r.checksum.clone()),
                        };
                        // TODO:
                        // let l_bigger = bytes_number <= l.checksum.bytes_number();
                        // let r_bigger = bytes_number <= r.checksum.bytes_number();
                        if l.checksum.matches(&r.checksum) {
                            network_verbose.matches.push(item);
                        } else {
                            result.success = false;
                            result.connections_ok = false;
                            network_verbose.checksum_mismatch.push(item);
                        }
                    }
                    (Some(l), None) => {
                        result.success = false;
                        result.connections_ok = false;
                        network_verbose.local_debugger_missing.push(NetworkMatches {
                            bytes_number,
                            timestamp,
                            local,
                            local_time: Some(l.timestamp),
                            local_crc64: Some(l.checksum.clone()),
                            remote,
                            remote_time: None,
                            remote_crc64: None,
                        });
                        break;
                    }
                    (None, Some(r)) => {
                        result.success = false;
                        result.connections_ok = false;
                        network_verbose.remote_debugger_missing.push(NetworkMatches {
                            bytes_number,
                            timestamp,
                            local,
                            local_time: None,
                            local_crc64: None,
                            remote,
                            remote_time: Some(r.timestamp),
                            remote_crc64: Some(r.checksum.clone()),
                        });
                        break;
                    }
                    (None, None) => {
                        result.success = false;
                        result.connections_ok = false;
                        network_verbose.both_debuggers_missing.push(NetworkMatches {
                            bytes_number,
                            timestamp,
                            local,
                            local_time: None,
                            local_crc64: None,
                            remote,
                            remote_time: None,
                            remote_crc64: None,
                        });
                        break;
                    }
                }
            }
            result.network_verbose.insert(ip, network_verbose);
        }

        self.test_result = Some(result);
    }
}
