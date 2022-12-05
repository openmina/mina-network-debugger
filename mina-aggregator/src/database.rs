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

use mina_recorder::{meshsub_stats::{Event, Hash}, custom_coding};

use super::rocksdb::{DbInner, DbError};

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
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub producer_id: PeerId,
    pub debugger_hostname: String,
    pub node_addr: SocketAddr,
}

impl GlobalEvent {
    pub fn new(event: Event, node_addr: SocketAddr, debugger_name: String) -> Option<Self> {
        if event.incoming {
            Some(GlobalEvent {
                producer_id: event.producer_id,
                hash: event.hash,
                block_height: event.block_height,
                global_slot: event.global_slot,
                debugger_name,
                received_message_id: Some(event.message_id),
                sent_message_id: None,
                time: event.time,
                latency: None,
                source_addr: Some(event.sender_addr),
                node_addr,
                destination_addr: None,
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
                time: event.time,
                latency: None,
                source_addr: None,
                node_addr,
                destination_addr: Some(event.receiver_addr),
            })
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

pub struct State {
    height: u32,
    last: BTreeMap<Key, GlobalEvent>,
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
            })),
            db: Arc::new(DbInner::open(path)?),
        })
    }

    pub fn post_data(&self, node_addr: SocketAddr, debugger_name: &str, event: Event) {
        log::info!("got data from {debugger_name} at {node_addr}");

        let current = event.block_height;

        let mut database_lock = self.cache.lock().expect("poisoned");
        if current < database_lock.height {
            return;
        } else if current > database_lock.height {
            database_lock.height = current;
            database_lock.last.clear();
        }

        let key = Key {
            producer_id: event.producer_id,
            debugger_hostname: debugger_name.to_owned(),
            node_addr,
        };

        if let Some(g_event) = database_lock.last.get_mut(&key) {
            if g_event.sent_message_id.is_none() {
                g_event.append(event);
            }
        } else if let Some(g_event) = GlobalEvent::new(event, node_addr, debugger_name.to_owned()) {
            database_lock.last.insert(key, g_event);
        }
        let events = database_lock.last.values().cloned().collect::<Vec<_>>();
        drop(database_lock);

        if let Err(err) = self.db.put_block(current, events) {
            log::error!("{err}");
        }
    }

    pub fn by_height(&self, height: u32) -> Option<Vec<GlobalEvent>> {
        match self.db.fetch_block(height) {
            Ok(v) => v,
            Err(err) => {
                log::error!("{err}");
                None
            }
        }
    }

    pub fn latest(&self) -> Option<(u32, Vec<GlobalEvent>)> {
        let lock = self.cache.lock().expect("poisoned");
        let events = lock.last.values().cloned().collect();

        Some((lock.height, events))
    }
}
