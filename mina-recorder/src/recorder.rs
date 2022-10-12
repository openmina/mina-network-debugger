use std::{collections::BTreeMap, time::SystemTime};

use super::{
    event::{EventMetadata, ConnectionInfo, DirectedId},
    connection::{HandleData, pnet, multistream_select, noise, mplex, mina_protocol},
    database::{DbFacade, DbGroup},
    tester::Tester,
    stats::Stats,
};

type Cn = pnet::State<Noise>;
type Noise = multistream_select::State<noise::State<Encrypted>>;
type Encrypted = multistream_select::State<mplex::State<Inner>>;
type Inner = multistream_select::State<mina_protocol::State>;

pub struct P2pRecorder {
    tester: Option<Tester>,
    chain_id: String,
    cns: BTreeMap<ConnectionInfo, (Cn, DbGroup)>,
    cx: Cx,
    apps: BTreeMap<u32, String>,
}

pub struct Cx {
    pub db: DbFacade,
    pub stats: Stats,
}

impl P2pRecorder {
    pub fn new(db: DbFacade, chain_id: String, test: bool) -> Self {
        P2pRecorder {
            tester: if test { Some(Tester::default()) } else { None },
            chain_id,
            cns: BTreeMap::default(),
            cx: Cx {
                db,
                stats: Stats::default(),
            },
            apps: BTreeMap::default(),
        }
    }

    pub fn on_alias(&mut self, pid: u32, alias: String) {
        self.apps.insert(pid, alias);
    }

    pub fn on_connect(&mut self, incoming: bool, metadata: EventMetadata) {
        if let Some(tester) = &mut self.tester {
            tester.on_connect(incoming, metadata);
            return;
        }
        let alias = self.apps.get(&metadata.id.pid).cloned().unwrap_or_default();
        let id = DirectedId {
            metadata,
            alias,
            incoming,
        };
        match self
            .cx
            .db
            .add(id.metadata.id.clone(), incoming, id.metadata.time)
        {
            Ok(group) => {
                log::info!("{id} {} new connection", group.id());

                self.cns
                    .insert(id.metadata.id, (Cn::new(self.chain_id.as_bytes()), group));
            }
            Err(err) => {
                log::error!("{id} new connection, cannot write in db {err}");
            }
        }
    }

    pub fn on_disconnect(&mut self, metadata: EventMetadata) {
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
        };
        if let Some((_, group)) = self.cns.remove(&id.metadata.id) {
            log::info!("{id} {} disconnect", group.id());
        }
    }

    #[rustfmt::skip]
    pub fn on_data(&mut self, incoming: bool, metadata: EventMetadata, mut bytes: Vec<u8>) {
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
