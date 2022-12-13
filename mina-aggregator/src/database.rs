use std::{
    sync::{Arc, Mutex},
    collections::BTreeMap,
    time::{SystemTime, Duration},
    net::SocketAddr,
    path::Path,
};

use radiation::{Absorb, Emit};
use serde::Serialize;
use libp2p_core::PeerId;

use mina_recorder::{
    meshsub_stats::{Event, Hash},
    custom_coding,
};

use super::rocksdb::{DbInner, DbError};

#[derive(Serialize, Clone, Absorb, Emit)]
pub struct GlobalBlockState {
    hash: Hash,
    events: Vec<GlobalEvent>,
}

#[derive(Serialize, Clone, Absorb, Emit)]
pub struct GlobalEvent {
    #[custom_absorb(custom_coding::peer_id_absorb)]
    #[custom_emit(custom_coding::peer_id_emit)]
    pub producer_id: PeerId,
    pub hash: Hash,
    pub block_height: u32,
    pub global_slot: u32,
    pub debugger_name: String,
    pub received_message_id: Option<u64>,
    pub sent_message_id: Option<u64>,
    #[custom_absorb(custom_coding::time_absorb)]
    #[custom_emit(custom_coding::time_emit)]
    pub time: SystemTime,
    #[custom_absorb(custom_coding::duration_opt_absorb)]
    #[custom_emit(custom_coding::duration_opt_emit)]
    pub latency: Option<Duration>,
    pub source_addr: Option<String>,
    #[custom_absorb(custom_coding::addr_absorb)]
    #[custom_emit(custom_coding::addr_emit)]
    pub node_addr: SocketAddr,
    pub destination_addr: Option<String>,
    pub node_id: u32,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub debugger_hostname: String,
    pub node_addr: SocketAddr,
}

impl GlobalEvent {
    pub fn new(event: Event, addr: SocketAddr, id: u32, debugger_name: String) -> Option<Self> {
        if event.incoming {
            Some(GlobalEvent {
                producer_id: event.producer_id,
                hash: event.hash,
                block_height: event.block_height,
                global_slot: event.global_slot,
                debugger_name,
                received_message_id: Some(event.message_id),
                sent_message_id: None,
                time: event.better_time,
                latency: None,
                source_addr: Some(event.sender_addr.to_string()),
                node_addr: addr,
                destination_addr: None,
                node_id: id,
            })
        } else {
            Some(GlobalEvent {
                producer_id: event.producer_id,
                hash: event.hash,
                block_height: event.block_height,
                global_slot: event.global_slot,
                debugger_name,
                received_message_id: None,
                sent_message_id: Some(event.message_id),
                time: event.better_time,
                latency: None,
                source_addr: None,
                node_addr: addr,
                destination_addr: Some(event.receiver_addr.to_string()),
                node_id: id,
            })
        }
    }

    pub fn append(&mut self, event: Event) {
        if let Some(latency) = event.latency {
            if !event.incoming {
                self.sent_message_id = Some(event.message_id);
                self.latency = Some(latency);
                self.destination_addr = Some(event.receiver_addr.to_string());
            }
        }
    }
}

pub struct State {
    height: u32,
    last: BTreeMap<Hash, BTreeMap<Key, GlobalEvent>>,
    ids: BTreeMap<SocketAddr, u32>,
    counter: u32,
}

#[derive(Clone)]
pub struct Database {
    cache: Arc<Mutex<State>>,
    db: Arc<DbInner>,
}

impl Database {
    pub fn open<P>(path: P) -> Result<Self, DbError>
    where
        P: AsRef<Path>,
    {
        Ok(Database {
            cache: Arc::new(Mutex::new(State {
                height: 0,
                last: BTreeMap::new(),
                ids: BTreeMap::new(),
                counter: 0,
            })),
            db: Arc::new(DbInner::open(path)?),
        })
    }

    pub fn post_data(&self, debugger_name: &str, event: Event) {
        let addr = event.node_address();

        log::info!("got data from {debugger_name} at {addr}");

        let current = event.block_height;

        let mut database_lock = self.cache.lock().expect("poisoned");
        if current < database_lock.height {
            return;
        } else if current > database_lock.height {
            database_lock.height = current;
            database_lock.last.clear();
        }

        let key = Key {
            debugger_hostname: debugger_name.to_owned(),
            node_addr: addr,
        };

        let id = if let Some(id) = database_lock.ids.get(&addr) {
            *id
        } else {
            let id = database_lock.counter;
            database_lock.ids.insert(addr, id);
            database_lock.counter += 1;
            id
        };

        let block_storage = database_lock.last.entry(event.hash).or_default();

        if let Some(g_event) = block_storage.get_mut(&key) {
            if g_event.sent_message_id.is_none() {
                g_event.append(event);
            }
        } else if let Some(g_event) = GlobalEvent::new(event, addr, id, debugger_name.to_owned()) {
            block_storage.insert(key, g_event);
        }

        let value = database_lock
            .last
            .iter()
            .map(|(&hash, events)| {
                let mut events = events.values().cloned().collect::<Vec<_>>();
                events.sort_by(|a, b| a.time.cmp(&b.time));
                GlobalBlockState { hash, events }
            })
            .collect::<Vec<_>>();
        drop(database_lock);

        if let Err(err) = self.db.put_block(current, value) {
            log::error!("{err}");
        }
    }

    pub fn by_height(&self, height: u32) -> Option<Vec<GlobalBlockState>> {
        match self.db.fetch_block(height) {
            Ok(v) => v,
            Err(err) => {
                log::error!("{err}");
                None
            }
        }
    }

    pub fn latest(&self) -> Option<(u32, Vec<GlobalBlockState>)> {
        let lock = self.cache.lock().expect("poisoned");
        let events = lock
            .last
            .iter()
            .map(|(&hash, events)| {
                let mut events = events.values().cloned().collect::<Vec<_>>();
                events.sort_by(|a, b| a.time.cmp(&b.time));
                GlobalBlockState { hash, events }
            })
            .collect();

        Some((lock.height, events))
    }
}
