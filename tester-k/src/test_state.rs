use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

use mina_ipc::message::ChecksumPair;

use crate::{
    constants,
    libp2p_helper::Process,
    message::{NetReport, PeerInfo, Registered, Report, DebuggerReport, Summary},
};

pub struct State {
    registry_ip: IpAddr,
    process: Process,
    build_number: u32,
    pub summary: BTreeMap<IpAddr, Summary>,
    pub test_result: Option<TestResult>,
}

#[derive(Serialize, Deserialize)]
pub struct TestResult {
    success: bool,
    debugger_version: Option<String>,
    verbose: Verbose,
}

#[derive(Default, Serialize, Deserialize)]
struct Verbose {
    ipc_matches: Vec<IpcTestResult>,
    ipc_mismatches: Vec<IpcTestResult>,
    network_verbose: NetworkVerbose,
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
    source_debugger_missing: Vec<NetworkMatches>,
    destination_debugger_missing: Vec<NetworkMatches>,
    both_debuggers_missing: Vec<NetworkMatches>,
}

#[derive(Serialize, Deserialize)]
struct NetworkMatches {
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

    pub fn new(registry_ip: IpAddr) -> Self {
        let (process, _) = Process::spawn();
        State {
            registry_ip,
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
            self.build_number = build_number;
            self.summary.clear();
            self.test_result = None;
        }
        let Some((peer_id, _, secret_key)) = self.process.generate_keypair()? else {
            return Err(anyhow::anyhow!("cannot generate key pair"));
        };

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
            debugger_version: None,
            verbose: Verbose::default(),
        };
        for (&ip, summary) in &self.summary {
            match (&summary.node, &summary.debugger) {
                (Some(s_node), Some(s_debugger)) => {
                    result.debugger_version = Some(s_debugger.version.clone());

                    let ipc_test_result = IpcTestResult {
                        ip,
                        node_crc64: s_node.ipc.clone(),
                        debugger_crc64: s_debugger.ipc.clone(),
                    };
                    if s_node.ipc.matches_(&s_debugger.ipc) {
                        result.verbose.ipc_matches.push(ipc_test_result);
                    } else {
                        result.success = false;
                        result.verbose.ipc_mismatches.push(ipc_test_result);
                    }

                    // for each connection seen by conntrack
                    // must exist only one debugger who seen this connection as incoming
                    // must exist only one (distinct) debugger who seen this connection as outgoing
                    let mut duplicate_track = BTreeSet::new();
                    for r in &summary.net_report {
                        let NetReport {
                            local,
                            remote,
                            timestamp,
                        } = *r;
                        // don't count connections to registry
                        // TODO: check ip also
                        let _ = self.registry_ip;
                        if remote.port() == constants::CENTER_PORT {
                            continue;
                        }
                        if !duplicate_track.insert((local, remote)) {
                            continue;
                        }

                        let p = self
                            .summary
                            .get(&local.ip())
                            .and_then(|dbg| dbg.debugger.as_ref())
                            .and_then(|report| report.network.get(&remote.ip()))
                            .map(|cn| (cn.timestamp, cn.checksum.clone()));
                        let (local_time, local_crc64) = if let Some((t, c)) = p {
                            (Some(t), Some(c))
                        } else {
                            (None, None)
                        };
                        let p = self
                            .summary
                            .get(&remote.ip())
                            .and_then(|dbg| dbg.debugger.as_ref())
                            .and_then(|report| report.network.get(&local.ip()))
                            .map(|cn| (cn.timestamp, cn.checksum.clone()));
                        // unstable
                        // .unzip();
                        let (remote_time, remote_crc64) = if let Some((t, c)) = p {
                            (Some(t), Some(c))
                        } else {
                            (None, None)
                        };

                        let entry = NetworkMatches {
                            timestamp,
                            local,
                            local_time,
                            local_crc64,
                            remote,
                            remote_time,
                            remote_crc64,
                        };
                        let bucket = &mut result.verbose.network_verbose;
                        let mut ok = false;
                        match (&entry.local_crc64, &entry.remote_crc64) {
                            (Some(src_crc64), Some(dst_crc64)) => {
                                if src_crc64.matches(dst_crc64) {
                                    ok = true;
                                    bucket.matches.push(entry);
                                } else {
                                    bucket.checksum_mismatch.push(entry);
                                }
                            }
                            (Some(_), None) => bucket.source_debugger_missing.push(entry),
                            (None, Some(_)) => bucket.destination_debugger_missing.push(entry),
                            (None, None) => bucket.both_debuggers_missing.push(entry),
                        }

                        result.success &= ok;
                    }
                }
                // ensure every node and every debugger already reported
                _ => return,
            }
        }

        self.summary.clear();
        self.test_result = Some(result);
    }
}
