use std::{
    sync::{Arc, Mutex},
    collections::BTreeMap,
    time::{SystemTime, Duration}, net::{SocketAddr, Ipv4Addr, IpAddr},
};

use mina_recorder::meshsub_stats::{BlockStat, Event};
use serde::Serialize;
use url::Url;
use libp2p_core::PeerId;

#[derive(Serialize, Clone)]
pub struct GlobalEvent {
    pub producer_id: PeerId,
    pub block_height: u32,
    pub global_slot: u32,
    pub debugger_url: String,
    pub received_message_id: u64,
    pub sent_message_id: Option<u64>,
    pub time: SystemTime,
    pub latency: Option<Duration>,
    pub source_addr: String,
    pub node_addr: SocketAddr,
    pub destination_addr: Option<String>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub producer_id: PeerId,
    pub debugger_hostname: String,
}

impl GlobalEvent {
    pub fn new(event: Event, node_addr: SocketAddr, debugger_url: Url) -> Option<Self> {
        if event.incoming {
            Some(GlobalEvent {
                producer_id: event.producer_id,
                block_height: event.block_height,
                global_slot: event.global_slot,
                debugger_url: debugger_url.to_string(),
                received_message_id: event.message_id,
                sent_message_id: None,
                time: event.time,
                latency: None,
                source_addr: event.sender_addr,
                node_addr,
                destination_addr: None,
            })
        } else {
            None
        }
    }

    pub fn append(&mut self, event: Event) {
        if let Some(latency) = event.latency {
            if !event.incoming {
                self.sent_message_id = Some(event.message_id);
                self.latency = Some(latency);
                self.destination_addr = Some(event.receiver_addr);
            }
        }
    }
}

#[derive(Default)]
pub struct State {
    blocks: BTreeMap<u32, BTreeMap<Key, GlobalEvent>>,
    debuggers: Vec<(u16, String, SocketAddr)>,
}

#[derive(Clone, Default)]
pub struct Database(Arc<Mutex<State>>);

impl Database {
    pub fn register_debugger(&self, ip: Option<IpAddr>, hostname: String, port: u16) {
        log::info!("register debugger: {hostname}:{port}");

        let ip = ip
            .or_else(|| {
                dns_lookup::lookup_host(&hostname)
                .ok()
                .and_then(|v| v.first().cloned())
            })
            .unwrap_or(Ipv4Addr::UNSPECIFIED.into());
        let node_addr = SocketAddr::new(ip, 8308); // TODO: get from debugger

        self.0
            .lock()
            .expect("poisoned")
            .debuggers
            .push((port, hostname, node_addr));
    }

    pub fn latest(&self) -> Option<(u32, Vec<GlobalEvent>)> {
        self.0
            .lock()
            .expect("poisoned")
            .blocks
            .iter()
            .rev()
            .next()
            .map(|(height, events)| (*height, events.values().cloned().collect()))
    }
}

pub struct Client {
    inner: reqwest::blocking::Client,
}

impl Client {
    pub fn new() -> Self {
        let inner = reqwest::blocking::ClientBuilder::new().build().unwrap();
        Client { inner }
    }

    pub fn refresh(&self, database: &Database) {
        let database_lock = database.0.lock().expect("poisoned");
        let debuggers = database_lock.debuggers.clone();
        drop(database_lock);
        for (port, hostname, node_addr) in debuggers {
            let scheme = if port == 443 { "https" } else { "http" };
            let url = Url::parse(&format!("{scheme}://{hostname}:{port}")).unwrap();
            let response = self
                .inner
                .get(url.join("block/latest").unwrap())
                .send()
                .unwrap();
            let item = serde_json::from_reader::<_, Option<BlockStat>>(response).unwrap();
            if let Some(item) = item {
                for event in item.events {
                    let key = Key {
                        producer_id: event.producer_id,
                        debugger_hostname: hostname.clone(),
                    };
                    let mut database_lock = database.0.lock().expect("poisoned");
                    let db_events = database_lock.blocks.entry(item.height).or_default();
                    if let Some(g_event) = db_events.get_mut(&key) {
                        if g_event.sent_message_id.is_none() {
                            g_event.append(event);
                        }
                    } else if let Some(g_event) = GlobalEvent::new(event, node_addr, url.clone()) {
                        db_events.insert(key, g_event);
                    }
                }
            }
        }
    }
}
