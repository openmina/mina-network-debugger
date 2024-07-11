use std::{
    path::Path,
    time::SystemTime,
    sync::{
        atomic::{
            AtomicU64,
            Ordering::{SeqCst, self},
        },
        Arc,
    },
    net::SocketAddr,
};

use itertools::Itertools;
use radiation::Emit;

use crate::{
    event::{ConnectionInfo, DirectedId},
    chunk::{ChunkHeader, EncryptionStatus},
    decode::{
        MessageType,
        meshsub_stats::{BlockStat, TxStat},
    },
    strace::StraceLine,
    meshsub_stats::Event,
};

use super::{
    core::{DbCore, DbError},
    types::{Connection, ConnectionId, Message, MessageId, StreamId, StreamKind, ConnectionStats},
};

pub struct DbFacade {
    cns: AtomicU64,
    pub messages: Arc<AtomicU64>,
    rnd_cnt: AtomicU64,
    inner: DbCore,
}

impl DbFacade {
    pub fn open<P>(path: P) -> Result<Self, DbError>
    where
        P: AsRef<Path>,
    {
        let inner = DbCore::open(path)?;

        Ok(DbFacade {
            cns: AtomicU64::new(inner.total::<{ DbCore::CONNECTIONS_CNT }>()?),
            messages: Arc::new(AtomicU64::new(inner.total::<{ DbCore::MESSAGES_CNT }>()?)),
            rnd_cnt: AtomicU64::new(inner.total::<{ DbCore::RANDOMNESS_CNT }>()?),
            inner,
        })
    }

    pub fn stats(
        &self,
        height: u32,
        node_address: SocketAddr,
        value: &BlockStat,
    ) -> Result<(), DbError> {
        self.inner
            .put_stats(height, node_address, value.chain(vec![]))
    }

    pub fn stats_block_v2(&self, event: Event) -> Result<(), DbError> {
        self.inner.put_stats_block_v2(event)
    }

    pub fn stats_tx(&self, height: u32, value: &TxStat) -> Result<(), DbError> {
        self.inner.put_stats_tx(height, value.chain(vec![]))
    }

    pub fn strace(&self) -> Result<DbStrace, DbError> {
        Ok(DbStrace {
            strace_cnt: AtomicU64::new(self.inner.total::<{ DbCore::STRACE_CNT }>()?),
            inner: self.inner.clone(),
        })
    }

    pub fn add(
        &self,
        info: ConnectionInfo,
        incoming: bool,
        alias: String,
        timestamp: SystemTime,
    ) -> Result<DbGroup, DbError> {
        let id = ConnectionId(self.cns.fetch_add(1, SeqCst));
        let addr = info.addr;
        let v = Connection {
            info,
            incoming,
            timestamp,
            stats_in: ConnectionStats::default(),
            stats_out: ConnectionStats::default(),
            timestamp_close: SystemTime::UNIX_EPOCH,
            alias,
        };
        self.inner.put_cn(id, v)?;
        self.inner.set_total::<{ DbCore::CONNECTIONS_CNT }>(id.0)?;

        Ok(DbGroup {
            addr,
            id,
            messages: self.messages.clone(),
            inner: self.inner.clone(),
        })
    }

    pub fn add_randomness(&self, bytes: [u8; 32]) -> Result<(), DbError> {
        let id = self.rnd_cnt.fetch_add(1, SeqCst);
        self.inner.put_randomness(id, bytes)?;

        Ok(())
    }

    pub fn core(&self) -> DbCore {
        self.inner.clone()
    }

    /// Warning, it will work wrong it the application will write messages from multiple threads
    /// It is ok for now.
    pub fn next_message_id(&self) -> u64 {
        self.messages.load(Ordering::SeqCst)
    }
}

pub struct DbStrace {
    strace_cnt: AtomicU64,
    inner: DbCore,
}

impl DbStrace {
    pub fn add_strace_line(&self, line: StraceLine) -> Result<(), DbError> {
        let id = self.strace_cnt.fetch_add(1, SeqCst);
        self.inner.put_strace(id, line.chain(vec![]))?;
        self.inner.set_total::<{ DbCore::STRACE_CNT }>(id)?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct DbGroup {
    addr: SocketAddr,
    id: ConnectionId,
    messages: Arc<AtomicU64>,
    inner: DbCore,
}

impl DbGroup {
    pub fn get(&self, id: StreamId) -> DbStream {
        DbStream {
            group: self.clone(),
            s_id: id,
        }
    }

    pub fn id(&self) -> ConnectionId {
        self.id
    }

    pub fn update(&self, stats: ConnectionStats, incoming: bool) -> Result<(), DbError> {
        let mut cn = self.inner.fetch_connection(self.id.0)?;
        if incoming {
            cn.stats_in += stats;
        } else {
            cn.stats_out += stats;
        }
        self.inner.put_cn(self.id, cn)
    }

    pub fn add_raw(
        &self,
        encryption_status: EncryptionStatus,
        incoming: bool,
        time: SystemTime,
        bytes: &[u8],
    ) -> Result<u64, DbError> {
        let header = ChunkHeader {
            size: bytes.len() as u32,
            time,
            encryption_status,
            incoming,
        };

        // TODO: avoid allocation
        let b = Vec::with_capacity(bytes.len() + ChunkHeader::SIZE);
        let mut b = header.chain(b);
        b.extend_from_slice(bytes);

        self.inner.put_blob(self.id, &b)
    }
}

impl Drop for DbGroup {
    fn drop(&mut self) {
        let id = self.id;
        if let Ok(mut cn) = self.inner.fetch_connection(id.0) {
            cn.timestamp_close = SystemTime::now();
            if let Err(err) = self.inner.put_cn(id, cn) {
                log::error!("connection {id}, error: {err}")
            }
        }
    }
}

#[derive(Clone)]
pub struct DbStream {
    group: DbGroup,
    s_id: StreamId,
}

impl DbStream {
    pub fn add(
        &self,
        did: &DirectedId,
        stream_kind: StreamKind,
        bytes: &[u8],
    ) -> Result<MessageId, DbError> {
        let index_ledger_hash = std::env::var("DEBUGGER_INDEX_LEDGER_HASH").is_ok();

        let offset = self.group.add_raw(
            EncryptionStatus::DecryptedNoise,
            did.incoming,
            did.metadata.time,
            bytes,
        )?;

        let mut ledger_hashes = vec![];
        let tys = match stream_kind {
            StreamKind::Unknown => vec![],
            StreamKind::Meshsub => {
                let (tys, hashes) = crate::decode::meshsub::parse_types(bytes, index_ledger_hash)?;
                ledger_hashes = hashes;
                tys
            }
            StreamKind::Kad => crate::decode::kademlia::parse_types(bytes)?,
            StreamKind::Handshake => crate::decode::noise::parse_types(bytes)?,
            StreamKind::Rpc => crate::decode::rpc::parse_types(bytes)?,
            StreamKind::IpfsId => vec![MessageType::Identify],
            StreamKind::IpfsPush => vec![MessageType::IdentifyPush],
            // TODO: message type (types)
            StreamKind::IpfsDelta => vec![],
            StreamKind::PeerExchange => vec![MessageType::PeerExchange],
            StreamKind::BitswapExchange => vec![MessageType::BitswapExchange],
            StreamKind::NodeStatus => vec![MessageType::NodeStatus],
            StreamKind::Select => vec![MessageType::Select],
            StreamKind::Mplex => vec![MessageType::Mplex],
            StreamKind::Yamux => vec![MessageType::Yamux],
        };

        let id = MessageId(self.group.messages.fetch_add(1, SeqCst));
        let v = Message {
            connection_id: self.group.id,
            stream_id: self.s_id,
            stream_kind,
            incoming: did.incoming,
            timestamp: did.metadata.time,
            offset,
            size: bytes.len() as u32,
            brief: tys.iter().map(|ty| ty.to_string()).join(","),
        };
        self.group
            .inner
            .put_message(&self.group.addr, id, v, tys, ledger_hashes)?;
        self.group
            .inner
            .set_total::<{ DbCore::MESSAGES_CNT }>(id.0)?;

        Ok(id)
    }
}
