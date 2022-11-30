use std::{
    sync::{Arc, Mutex},
    collections::BTreeMap,
    time::{SystemTime, Duration},
    net::SocketAddr,
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
    pub fn from_event(event: Event, node_addr: SocketAddr, debugger_url: String) -> Option<Self> {
        if event.incoming {
            Some(GlobalEvent {
                producer_id: event.producer_id,
                block_height: event.block_height,
                global_slot: event.global_slot,
                debugger_url,
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
    debuggers: Vec<(SocketAddr, String)>,
}

#[derive(Clone, Default)]
pub struct Database(Arc<Mutex<State>>);

impl Database {
    pub fn register_debugger(&self, alias: String, address: SocketAddr) {
        log::info!("register debugger: {alias} at {address}");
        self.0
            .lock()
            .expect("poisoned")
            .debuggers
            .push((address, alias));
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
        for (addr, hostname) in debuggers {
            let port = addr.port();
            let node_addr = {
                let mut a = addr;
                a.set_port(8303); // TODO: from debugger
                a
            };
            let scheme = if port == 443 { "https" } else { "http" };
            let debugger_url = Url::parse(&format!("{scheme}://{hostname}:{port}")).unwrap();
            let response = self
                .inner
                .get(debugger_url.join("block/latest").unwrap())
                .send()
                .unwrap();
            let item = serde_json::from_reader::<_, Option<BlockStat>>(response).unwrap();
            if let Some(item) = item {
                for mut event in item.events {
                    if event.incoming {
                        event.receiver_addr = hostname.clone();
                    } else {
                        event.sender_addr = hostname.clone();
                    }
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
                    } else if let Some(g_event) =
                        GlobalEvent::from_event(event, node_addr, debugger_url.to_string())
                    {
                        db_events.insert(key, g_event);
                    }
                    drop(database_lock);
                }
            }
        }
    }
}
