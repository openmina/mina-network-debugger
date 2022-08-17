use std::collections::{BTreeMap, VecDeque};

use super::{
    EventMetadata, ConnectionInfo, DirectedId,
    connection::{HandleData, pnet, multistream_select, noise, mplex, mina_protocol},
    database::{DbFacade, DbGroup},
};

type Cn = pnet::State<Noise>;
type Noise = multistream_select::State<noise::State<Encrypted>>;
type Encrypted = multistream_select::State<mplex::State<Inner>>;
type Inner = multistream_select::State<mina_protocol::State>;

pub struct P2pRecorder {
    chain_id: String,
    db: DbFacade,
    cns: BTreeMap<ConnectionInfo, (Cn, DbGroup)>,
    cx: Cx,
    apps: BTreeMap<u32, String>,
}

#[derive(Default)]
pub struct Cx {
    randomness: VecDeque<[u8; 32]>,
}

impl Cx {
    pub fn push_randomness(&mut self, bytes: [u8; 32]) {
        self.randomness.push_back(bytes);
    }

    pub fn iter_rand(&self) -> impl Iterator<Item = &[u8; 32]> + '_ {
        self.randomness.iter().rev()
    }
}

impl P2pRecorder {
    pub fn new(db: DbFacade, chain_id: String) -> Self {
        P2pRecorder {
            chain_id,
            db,
            cns: BTreeMap::default(),
            cx: Cx::default(),
            apps: BTreeMap::default(),
        }
    }

    pub fn on_alias(&mut self, pid: u32, alias: String) {
        self.apps.insert(pid, alias);
    }

    pub fn on_connect(&mut self, incoming: bool, metadata: EventMetadata) {
        let alias = self.apps.get(&metadata.id.pid).cloned().unwrap_or_default();
        let ConnectionInfo { addr, pid, fd } = &metadata.id;
        if incoming {
            log::info!("{alias}_{pid} accept {addr} {fd}");
        } else {
            log::info!("{alias}_{pid} connect {addr} {fd}");
        }
        match self.db.add(metadata.id.clone(), incoming, metadata.time) {
            Ok(group) => {
                self.cns.insert(metadata.id, (Cn::new(self.chain_id.as_bytes()), group));
            },
            Err(err) => {
                log::error!("cannot process connection: {err}");
            }
        }
    }

    pub fn on_disconnect(&mut self, metadata: EventMetadata) {
        let alias = self.apps.get(&metadata.id.pid).cloned().unwrap_or_default();
        let ConnectionInfo { addr, pid, fd } = &metadata.id;
        log::info!("{alias}_{pid} disconnect {addr} {fd}");
        self.cns.remove(&metadata.id);
    }

    #[rustfmt::skip]
    pub fn on_data(&mut self, incoming: bool, metadata: EventMetadata, mut bytes: Vec<u8>) {
        if let Some((cn, group)) = self.cns.get_mut(&metadata.id) {
            let alias = self.apps.get(&metadata.id.pid).cloned().unwrap_or_default();
            let id = DirectedId {
                metadata,
                alias,
                incoming,
            };
            if let Err(err) = cn.on_data(id.clone(), &mut bytes, &mut self.cx, &*group) {
                log::error!("{id}: {err}");
            }
        }
    }

    pub fn on_randomness(&mut self, _pid: u32, bytes: [u8; 32]) {
        // log::info!("{alias} random: {}", hex::encode(bytes));
        self.cx.push_randomness(bytes);
    }
}
