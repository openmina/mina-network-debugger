use std::{
    collections::BTreeMap,
    time::SystemTime,
    net::{SocketAddr, IpAddr},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
};

use serde::Serialize;
use parking_lot::Mutex;

use super::{
    event::{EventMetadata, ConnectionInfo, DirectedId},
    connection::{HandleData, pnet, multistream_select, noise, mux, mina_protocol},
    database::{DbFacade, DbGroup},
    tester::Tester,
    stats::{Stats, StatsState},
};

type Cn = pnet::State<Noise>;
type Noise = multistream_select::State<noise::State<Encrypted>>;
type Encrypted = multistream_select::State<mux::State<Inner>>;
type Inner = multistream_select::State<mina_protocol::State>;

pub struct P2pRecorder {
    tester: Option<Tester>,
    cns: BTreeMap<ConnectionInfo, ThreadContext>,
    cns_main_thread: BTreeMap<ConnectionInfo, ConnectionContext>,
    // this is used by capnp reader
    // TODO: split
    pub cx: Arc<Cx>,
}

pub struct ThreadContext {
    handle: JoinHandle<()>,
    tx: mpsc::Sender<NetworkChunk>,
}

pub struct ConnectionContext {
    cn: Cn,
    db: DbGroup,
}

pub struct NetworkChunk {
    pub metadata: EventMetadata,
    pub data: Vec<u8>,
    pub incoming: bool,
    pub buffered: usize,
}

// my local sandbox
// /coda/0.0.1/dd0f3f26be5a093f00077d1cd5d89abc253c95f301e9c12ae59e2d7c6052cc4d
const CHAINS: [(&str, &str); 3] = [
    (
        "mainnet",
        "/coda/0.0.1/5f704cc0c82e0ed70e873f0893d7e06f148524e3f0bdae2afb02e7819a0c24d1",
    ),
    (
        "devnet",
        "/coda/0.0.1/b6ee40d336f4cc3f33c1cc04dee7618eb8e556664c2b2d82ad4676b512a82418",
    ),
    (
        "berkeley",
        "/coda/0.0.1/fb30d090bb37e8aa354114d8c794b0f7072648a67bd1a08613684ac6f7c86028",
    ),
];

pub struct Cx {
    pub apps: Mutex<BTreeMap<u32, (String, SocketAddr)>>,
    pub stats_state: Mutex<BTreeMap<SocketAddr, StatsState>>,
    pub db: DbFacade,
    pub stats: Stats,
    pub aggregator: Option<Aggregator>,
}

impl Cx {
    pub fn pid_to_addr(&self, pid: u32) -> SocketAddr {
        self.apps
            .lock()
            .get(&pid)
            .as_ref()
            .map(|(_, addr)| addr.clone())
            .unwrap_or(SocketAddr::new(IpAddr::V4(0.into()), 0))
    }
}

#[derive(Clone)]
pub struct Aggregator {
    pub client: reqwest::blocking::Client,
    pub url: reqwest::Url,
    pub debugger_name: String,
}

impl Aggregator {
    pub fn post_event<T>(&self, event: T)
    where
        T: Serialize,
    {
        let url = self.url.clone();
        let body = format!(
            "{{\"alias\": \"{}\", \"event\": {} }}",
            self.debugger_name,
            serde_json::to_string(&event).unwrap(),
        );
        if let Err(err) = self.client.post(url).body(body).send() {
            log::error!("failed to post event on aggregator {err}");
        }
    }
}

impl P2pRecorder {
    pub fn new(db: DbFacade, test: bool) -> Self {
        use std::env;

        let aggregator = if let Ok(aggregator_str) = env::var("AGGREGATOR") {
            log::info!("use aggregator {aggregator_str}");
            if let Ok(aggregator) = aggregator_str.parse::<reqwest::Url>() {
                let debugger_name = env::var("DEBUGGER_NAME").unwrap_or("noname".to_owned());
                let client = reqwest::blocking::Client::new();
                let url = aggregator.join("new").unwrap();
                // let body = format!("{{\"alias\": {hostname:?}, \"port\": {port} }}");
                // match client.post(url).body(body).send() {
                //     Ok(_) => (),
                //     Err(err) => log::error!("cannot register at aggregator: {err}"),
                // }
                Some(Aggregator {
                    client,
                    url,
                    debugger_name,
                })
            } else {
                log::error!("cannot parse aggregator url {aggregator_str}");
                None
            }
        } else {
            None
        };

        P2pRecorder {
            tester: if test { Some(Tester::default()) } else { None },
            cns: BTreeMap::default(),
            cns_main_thread: BTreeMap::default(),
            cx: Arc::new(Cx {
                apps: Mutex::default(),
                db,
                stats: Stats::default(),
                stats_state: Mutex::default(),
                aggregator,
            }),
        }
    }

    pub fn set_port(&mut self, pid: u32, port: u16) {
        self.cx
            .apps
            .lock()
            .get_mut(&pid)
            .map(|(_, addr)| addr.set_port(port));
    }

    pub fn on_alias(&mut self, pid: u32, alias: String) {
        let ip = alias
            .split('-')
            .nth(1)
            .unwrap_or("0.0.0.0")
            .parse()
            .unwrap_or(IpAddr::V4(0.into()));
        self.cx
            .apps
            .lock()
            .insert(pid, (alias, SocketAddr::new(ip, 8302)));
    }

    pub fn on_connect<const MAIN_THREAD: bool>(
        &mut self,
        incoming: bool,
        metadata: EventMetadata,
        buffered: usize,
    ) {
        if let Some(tester) = &mut self.tester {
            tester.on_connect(incoming, metadata);
            return;
        }
        let alias = {
            let lock = self.cx.apps.lock();
            lock.get(&metadata.id.pid)
                .cloned()
                .map(|(a, _)| a)
                .unwrap_or_default()
        };
        let mut it = alias.split('-');
        let network = it.next().expect("`split` must yield at least one");
        let chain_id = CHAINS
            .iter()
            .find_map(|(k, v)| if *k == network { Some(*v) } else { None })
            .unwrap_or(network);
        let id = DirectedId {
            metadata,
            alias: alias.clone(),
            incoming,
            buffered,
        };
        match self.cx.db.add(
            id.metadata.id.clone(),
            incoming,
            id.alias.clone(),
            id.metadata.time,
        ) {
            Ok(group) => {
                log::debug!("{id} {} new connection", group.id());
                let info = id.metadata.id.clone();

                let (tx, rx) = mpsc::channel();
                let cx = self.cx.clone();
                let mut cn = Cn::new(chain_id.as_bytes());

                if MAIN_THREAD {
                    self.cns_main_thread.insert(
                        id.metadata.id,
                        ConnectionContext {
                            cn: Cn::new(chain_id.as_bytes()),
                            db: group,
                        },
                    );

                    return;
                }

                let handle = thread::spawn(move || {
                    while let Ok(NetworkChunk {
                        metadata,
                        mut data,
                        incoming,
                        buffered,
                    }) = rx.recv()
                    {
                        let alias = {
                            let lock = cx.apps.lock();
                            lock.get(&metadata.id.pid)
                                .cloned()
                                .map(|(a, _)| a)
                                .unwrap_or_default()
                        };
                        let id = DirectedId {
                            metadata,
                            alias,
                            incoming,
                            buffered,
                        };
                        if let Err(err) = cn.on_data(id.clone(), &mut data, &cx, &group) {
                            log::error!("{id}: {err}");
                        }
                    }
                    log::debug!("{id} {} disconnect", group.id());
                });
                let t_cx = ThreadContext { handle, tx };

                self.cns.insert(info, t_cx);
            }
            Err(err) => {
                log::error!("{id} new connection, cannot write in db {err}");
            }
        }
    }

    pub fn on_disconnect(&mut self, metadata: EventMetadata, buffered: usize) {
        if let Some(tester) = &mut self.tester {
            tester.on_disconnect(metadata);
            return;
        }
        let alias = {
            let lock = self.cx.apps.lock();
            lock.get(&metadata.id.pid)
                .cloned()
                .map(|(a, _)| a)
                .unwrap_or_default()
        };
        let incoming = false; // warning, we really don't know at this point
        let id = DirectedId {
            metadata,
            alias,
            incoming,
            buffered,
        };
        if let Some(t_cx) = self.cns.remove(&id.metadata.id) {
            drop(t_cx.tx);
            match t_cx.handle.join() {
                Ok(()) => log::info!("{id} join thread"),
                Err(err) => log::error!("{id} {err:?}"),
            }
        } else if let Some(cn_cx) = self.cns_main_thread.remove(&id.metadata.id) {
            log::info!("{id} {} disconnect", cn_cx.db.id());
        }
    }

    #[rustfmt::skip]
    pub fn on_data(
        &mut self,
        incoming: bool,
        metadata: EventMetadata,
        buffered: usize,
        mut bytes: Vec<u8>,
    ) {
        if let Some(tester) = &mut self.tester {
            tester.on_data(incoming, metadata, bytes);
            return;
        }
        if let Some(t_cx) = self.cns.get_mut(&metadata.id) {
            t_cx.tx.send(NetworkChunk {
                metadata,
                data: bytes,
                incoming,
                buffered,
            }).unwrap_or_default();
        } else if let Some(cn_cx) = self.cns_main_thread.get_mut(&metadata.id) {
            let alias = {
                let lock = self.cx.apps.lock();
                lock.get(&metadata.id.pid)
                    .cloned()
                    .map(|(a, _)| a)
                    .unwrap_or_default()
            };
            let id = DirectedId {
                metadata,
                alias,
                incoming,
                buffered,
            };
            if let Err(err) = cn_cx.cn.on_data(id.clone(), &mut bytes, &self.cx, &cn_cx.db) {
                log::error!("{id}: {err}");
            }
        }
    }

    pub fn on_randomness(&mut self, pid: u32, bytes: Vec<u8>, time: SystemTime) {
        use time::OffsetDateTime;

        let (hour, minute, second, nano) = OffsetDateTime::from(time).time().as_hms_nano();
        log::debug!(
            "{hour:02}:{minute:02}:{second:02}:{nano:09} {pid} random: {} {}",
            bytes.len(),
            hex::encode(&bytes),
        );
        if let Err(err) = self.cx.db.add_randomness(bytes) {
            log::error!("failed to store randomness: {err}");
        }
    }
}
