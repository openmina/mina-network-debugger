use std::{
    path::Path,
    time::SystemTime,
    sync::{
        atomic::{AtomicU64, Ordering::SeqCst},
        Arc,
    }, net::SocketAddr,
};

use radiation::Emit;

use crate::{
    event::{ConnectionInfo, ChunkHeader, EncryptionStatus},
    decode::MessageType, strace::StraceLine,
};

use super::{
    core::{DbCore, DbError},
    types::{Connection, ConnectionId, StreamFullId, Message, MessageId, StreamId, StreamKind},
};

pub struct DbFacade {
    cns: AtomicU64,
    messages: Arc<AtomicU64>,
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
        timestamp: SystemTime,
    ) -> Result<DbGroup, DbError> {
        let id = ConnectionId(self.cns.fetch_add(1, SeqCst));
        let addr = info.addr;
        let v = Connection {
            info,
            incoming,
            timestamp,
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

    pub fn add_randomness(&self, bytes: Vec<u8>) -> Result<(), DbError> {
        let id = self.rnd_cnt.fetch_add(1, SeqCst);
        self.inner.put_randomness(id, bytes)?;

        Ok(())
    }

    pub fn core(&self) -> DbCore {
        self.inner.clone()
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

pub struct DbGroup {
    addr: SocketAddr,
    id: ConnectionId,
    messages: Arc<AtomicU64>,
    inner: DbCore,
}

impl DbGroup {
    pub fn add(&self, id: StreamId, kind: StreamKind) -> DbStream {
        DbStream {
            addr: self.addr,
            id: StreamFullId { cn: self.id, id },
            kind,
            messages: self.messages.clone(),
            inner: self.inner.clone(),
        }
    }

    pub fn id(&self) -> ConnectionId {
        self.id
    }

    pub fn add_raw(
        &self,
        encryption_status: EncryptionStatus,
        incoming: bool,
        time: SystemTime,
        bytes: &[u8],
    ) -> Result<(), DbError> {
        let header = ChunkHeader {
            size: bytes.len() as u32,
            time,
            encryption_status,
            incoming,
        };

        let b = Vec::with_capacity(bytes.len() + ChunkHeader::SIZE);
        let mut b = header.chain(b);
        b.extend_from_slice(bytes);

        let sb = self.inner.get_raw_stream(self.id)?;
        let mut file = sb.lock().expect("poisoned");
        let _ = file.write(&b).map_err(|err| DbError::IoCn(self.id, err))?;

        Ok(())
    }
}

impl Drop for DbGroup {
    fn drop(&mut self) {
        self.inner.remove_raw_stream(self.id);
    }
}

pub struct DbStream {
    addr: SocketAddr,
    id: StreamFullId,
    kind: StreamKind,
    messages: Arc<AtomicU64>,
    inner: DbCore,
}

impl Drop for DbStream {
    fn drop(&mut self) {
        self.inner.remove_stream(self.id);
    }
}

impl DbStream {
    pub fn add(&self, incoming: bool, timestamp: SystemTime, bytes: &[u8]) -> Result<(), DbError> {
        let sb = self.inner.get_stream(self.id)?;
        let mut file = sb.lock().expect("poisoned");
        let offset = file.write(bytes).map_err(|err| DbError::Io(self.id, err))?;
        drop(file);

        let tys = match self.kind {
            StreamKind::Unknown => vec![],
            StreamKind::Meshsub => crate::decode::meshsub::parse_types(bytes)?,
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
        };

        let id = MessageId(self.messages.fetch_add(1, SeqCst));
        let v = Message {
            connection_id: self.id.cn,
            stream_id: self.id.id,
            stream_kind: self.kind,
            incoming,
            timestamp,
            offset,
            size: bytes.len() as u32,
        };
        self.inner.put_message(&self.addr, id, v, tys)?;
        self.inner.set_total::<{ DbCore::MESSAGES_CNT }>(id.0)?;

        Ok(())
    }
}
