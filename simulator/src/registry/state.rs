use std::{
    net::{SocketAddr, IpAddr},
    collections::BTreeMap,
};
use tokio::sync::{mpsc, oneshot};

use super::messages::{Registered, Summary, PeerInfo, NetReport, MockReport, DebuggerReport, MockSplitReport};
use crate::{libp2p_helper::Process, peer::split_behavior};

#[derive(Default)]
pub struct SplitContext {
    thread: Option<mpsc::UnboundedSender<(IpAddr, oneshot::Sender<()>)>>,
}

impl SplitContext {
    pub fn request(&mut self, registered: usize, fr: IpAddr) -> oneshot::Receiver<()> {
        use tokio::time;
        use reqwest::blocking::ClientBuilder;

        let (tx, rx) = oneshot::channel();
        let sender = self.thread.get_or_insert_with(|| {
            let (ttx, mut trx) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                let client = ClientBuilder::new().timeout(split_behavior::time::REGISTRY_TIMEOUT).build()?;
                let mut requested = BTreeMap::<_, oneshot::Sender<()>>::default();
                loop {
                    match time::timeout(split_behavior::time::WAIT_TIMEOUT, trx.recv()).await {
                        Ok(Some((addr, tx))) => {
                            requested.insert(SocketAddr::new(addr, split_behavior::PEER_PORT), tx);
                            if requested.len() == registered {
                                break;
                            }
                        }
                        _ => break,
                    }
                }

                let left = requested.keys().take(requested.len() / 2).cloned().collect::<Vec<_>>();
                let right = requested.keys().skip(requested.len() / 2).cloned().collect::<Vec<_>>();

                let keys = requested.keys().cloned().collect::<Vec<_>>();
                for tx in requested.into_values() {
                    tx.send(()).unwrap_or_default();
                }
                time::sleep(split_behavior::time::SHIFT).await;

                for target in &keys {
                    let whitelist = if left.contains(target) {
                        &left
                    } else {
                        &right
                    };
                    let url = format!("http://{}:8000/firewall/whitelist/enable", target.ip());
                    let whitelist = serde_json::to_string(&whitelist).unwrap();
                    log::info!("POST {url}, body: {whitelist}");
                    match client.post(url).body(whitelist).send() {
                        Ok(response) => log::info!("{}", response.status()),
                        Err(err) => log::error!("{err}"),
                    }
                }
                time::sleep(split_behavior::time::SPLIT).await;
                for target in &keys {
                    let url = format!("http://{}:8000/firewall/whitelist/disable", target.ip());
                    log::info!("POST {url}");
                    match client.post(url).send() {
                        Ok(response) => log::info!("{}", response.status()),
                        Err(err) => log::error!("{err}"),
                    }
                }

                Ok::<_, anyhow::Error>(())
            });
            ttx
        });
        sender.send((fr, tx)).unwrap_or_default();

        rx
    }
}

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
