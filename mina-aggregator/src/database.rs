use std::{sync::{Arc, Mutex}, collections::BTreeMap, time::SystemTime};

use mina_recorder::meshsub_stats::{BlockStat, Event};
use url::Url;
use serde::Deserialize;

type State = BTreeMap<u32, BTreeMap<Key, Event>>;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    time: SystemTime,
    sender_addr: String,
    receiver_addr: String,
}

#[derive(Clone, Default)]
pub struct Database(Arc<Mutex<State>>);

impl Database {
    pub fn latest(&self) -> Option<BlockStat> {
        self.0.lock().unwrap().iter().rev().next().map(|(height, events)| BlockStat {
            height: *height,
            events: events.values().cloned().collect(),
        })
    }
}

#[derive(Deserialize)]
pub struct Config {
    debuggers: Vec<Url>,
}

pub struct Client {
    inner: reqwest::blocking::Client,
    config: Config,
}

impl Client {
    pub fn new(config: Config) -> Self {
        let inner = reqwest::blocking::ClientBuilder::new().build().unwrap();
        Client { inner, config }
    }

    pub fn refresh(&self, database: &Database) {
        for url in &self.config.debuggers {
            let response = self.inner.get(url.join("/block/latest").unwrap()).send().unwrap();
            let item = serde_json::from_reader::<_, Option<BlockStat>>(response).unwrap();
            if let Some(item) = item {
                for mut event in item.events {
                    if event.incoming {
                        event.receiver_addr = url.to_string();
                    } else {
                        event.sender_addr = url.to_string();
                    }
                    let key = Key {
                        time: event.time,
                        sender_addr: event.sender_addr.clone(),
                        receiver_addr: event.receiver_addr.clone(),
                    };
                    let mut database = database.0.lock().expect("poisoned");
                    let db_events = database.entry(item.height).or_default();
                    db_events.insert(key, event);
                }
            }
        }
    }
}
