use std::{
    sync::{Arc, Mutex},
    collections::BTreeMap,
    time::{SystemTime, Duration},
    net::{SocketAddr, Ipv4Addr, IpAddr},
};

use mina_recorder::meshsub_stats::Event;
use serde::Serialize;
use libp2p_core::PeerId;

#[derive(Serialize, Clone)]
pub struct GlobalEvent {
    pub producer_id: PeerId,
    pub block_height: u32,
    pub global_slot: u32,
    pub debugger_name: String,
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
    pub fn new(event: Event, node_addr: SocketAddr, debugger_name: String) -> Option<Self> {
        if event.incoming {
            Some(GlobalEvent {
                producer_id: event.producer_id,
                block_height: event.block_height,
                global_slot: event.global_slot,
                debugger_name,
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
}

#[derive(Clone, Default)]
pub struct Database(Arc<Mutex<State>>);

impl Database {
    pub fn post_data(&self, ip: Option<IpAddr>, debugger_name: &str, event: Event) {
        let ip = ip.unwrap_or_else(|| Ipv4Addr::UNSPECIFIED.into());
        log::info!("got data from {debugger_name} at {ip}");

        let node_addr = SocketAddr::new(ip, 8308); // TODO: get from debugger

        let key = Key {
            producer_id: event.producer_id,
            debugger_hostname: debugger_name.to_owned(),
        };

        let mut database_lock = self.0.lock().expect("poisoned");
        let db_events = database_lock.blocks.entry(event.block_height).or_default();
        if let Some(g_event) = db_events.get_mut(&key) {
            if g_event.sent_message_id.is_none() {
                g_event.append(event);
            }
        } else if let Some(g_event) = GlobalEvent::new(event, node_addr, debugger_name.to_owned()) {
            db_events.insert(key, g_event);
        }
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
