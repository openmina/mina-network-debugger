use std::{collections::BTreeMap, time::SystemTime};

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
    cns: BTreeMap<ConnectionInfo, (Cn, DbGroup)>,
    cx: Cx,
    apps: BTreeMap<u32, String>,
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
    pub db: DbFacade,
    pub stats: Stats,
    pub stats_state: StatsState,
}

impl P2pRecorder {
    pub fn new(db: DbFacade, test: bool) -> Self {
        P2pRecorder {
            tester: if test { Some(Tester::default()) } else { None },
            cns: BTreeMap::default(),
            cx: Cx {
                db,
                stats: Stats::default(),
                stats_state: StatsState::default(),
            },
            apps: BTreeMap::default(),
        }
    }

    pub fn on_alias(&mut self, pid: u32, alias: String) {
        self.apps.insert(pid, alias);
    }

    pub fn on_connect(&mut self, incoming: bool, metadata: EventMetadata, buffered: usize) {
        if let Some(tester) = &mut self.tester {
            tester.on_connect(incoming, metadata);
            return;
        }
        let alias = self.apps.get(&metadata.id.pid).cloned().unwrap_or_default();
        let mut it = alias.split('-');
        let network = it.next().expect("`split` must yield at least one");
        let chain_id = CHAINS
            .iter()
            .find_map(|(k, v)| if *k == network { Some(*v) } else { None })
            .unwrap_or(CHAINS[0].1);
        let id = DirectedId {
            metadata,
            alias,
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

                self.cns
                    .insert(id.metadata.id, (Cn::new(chain_id.as_bytes()), group));
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
        let alias = self.apps.get(&metadata.id.pid).cloned().unwrap_or_default();
        let incoming = false; // warning, we really don't know at this point
        let id = DirectedId {
            metadata,
            alias,
            incoming,
            buffered,
        };
        if let Some((_, group)) = self.cns.remove(&id.metadata.id) {
            log::debug!("{id} {} disconnect", group.id());
        }
    }

    #[rustfmt::skip]
    pub fn on_data(&mut self, incoming: bool, metadata: EventMetadata, buffered: usize, mut bytes: Vec<u8>) {
        if let Some(tester) = &mut self.tester {
            tester.on_data(incoming, metadata, bytes);
            return;
        }
        if let Some((cn, group)) = self.cns.get_mut(&metadata.id) {
            let alias = self.apps.get(&metadata.id.pid).cloned().unwrap_or_default();
            let id = DirectedId {
                metadata,
                alias,
                incoming,
                buffered,
            };
            if let Err(err) = cn.on_data(id.clone(), &mut bytes, &mut self.cx, &*group) {
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
