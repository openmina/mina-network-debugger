use std::{
    net::{SocketAddr, IpAddr},
    collections::BTreeMap,
};

use super::messages::{Registered, Summary, PeerInfo, NetReport, MockReport, DebuggerReport, MockSplitReport};
use crate::libp2p_helper::Process;

#[derive(Default)]
pub struct State {
    process: Option<Process>,
    build_number: u32,
    last: Option<IpAddr>,
    summary: BTreeMap<IpAddr, Summary>,
}

impl State {
    pub fn summary(&self) -> &BTreeMap<IpAddr, Summary> {
        &self.summary
    }

    pub fn build_number(&self) -> u32 {
        self.build_number
    }

    pub fn peers(&self) -> Vec<IpAddr> {
        self.summary.keys().cloned().collect()
    }

    pub fn register(&mut self, addr: SocketAddr, build_number: u32, nodes: u32) -> anyhow::Result<Registered> {
        let process = self.process.get_or_insert_with(|| match Process::spawn() {
            (v, _) => v,
        });

        if self.build_number != build_number {
            log::info!(
                "current build: {}, this build: {}, cleanup",
                self.build_number,
                build_number,
            );
            self.build_number = build_number;
            self.summary.clear();
        }
        let Some((peer_id, _, secret_key)) = process.generate_keypair()? else {
            return Err(anyhow::anyhow!("cannot generate key pair"));
        };
        let already_registered = self.summary.len() as u32;
        log::debug!("already registered {already_registered} nodes, new peer_id {peer_id}");

        let info = PeerInfo {
            ip: addr.ip(),
            peer_id: peer_id.clone(),
        };
        let response = Registered {
            info: info.clone(),
            secret_key: hex::encode(secret_key),
            external: addr.ip(),
            prev: self.last,
            peers: self
                .summary
                .iter()
                // .take(Self::SEED_NODES)
                .map(|(k, s)| (*k, s.peer_id.clone()))
                .collect(),
            // last registered node is a leader
            leader: already_registered == nodes - 1,
        };
        self.summary.insert(
            addr.ip(),
            Summary {
                peer_id,
                ..Default::default()
            },
        );
        self.last = Some(addr.ip());

        Ok(response)
    }

    pub fn add_net_report(&mut self, addr: SocketAddr, report: Vec<NetReport>) {
        if let Some(summary) = self.summary.get_mut(&addr.ip()) {
            summary.net_report = report;
        }
    }

    pub fn add_mock_report(&mut self, addr: SocketAddr, report: MockReport) {
        if let Some(summary) = self.summary.get_mut(&addr.ip()) {
            summary.mock_report = Some(report);
        }
    }

    pub fn add_mock_split_report(&mut self, addr: SocketAddr, report: MockSplitReport) {
        if let Some(summary) = self.summary.get_mut(&addr.ip()) {
            summary.mock_split_report = Some(report);
        }
    }

    pub fn add_debugger_report(&mut self, addr: SocketAddr, report: DebuggerReport) {
        if let Some(summary) = self.summary.get_mut(&addr.ip()) {
            summary.debugger_report = Some(report);
        }
    }

    pub fn reset(&mut self) {
        self.last = None;
        self.summary.clear();
    }
}
