use std::{
    path::Path,
    time::SystemTime,
    sync::{
        atomic::{AtomicU64, Ordering::SeqCst},
        Arc,
    },
};

use crate::ConnectionInfo;

use super::core::{DbCore, DbError};
use super::types::{
    Connection, ConnectionId, Stream, StreamId, Message, MessageId, StreamMeta, StreamKind,
};

pub struct DbFacade {
    cns: AtomicU64,
    streams: Arc<AtomicU64>,
    messages: Arc<AtomicU64>,
    inner: DbCore,
}

impl DbFacade {
    pub fn open<P>(path: P) -> Result<Self, DbError>
    where
        P: AsRef<Path>,
    {
        let inner = DbCore::open(path)?;

        Ok(DbFacade {
            cns: AtomicU64::new(inner.total(0)?),
            streams: Arc::new(AtomicU64::new(inner.total(1)?)),
            messages: Arc::new(AtomicU64::new(inner.total(2)?)),
            inner,
        })
    }

    pub fn add(
        &self,
        info: ConnectionInfo,
        incoming: bool,
        timestamp: SystemTime,
    ) -> Result<DbGroup, DbError> {
        let id = ConnectionId(self.cns.fetch_add(1, SeqCst));
        let v = Connection {
            info,
            incoming,
            timestamp,
        };
        self.inner.put_cn(id, v)?;
        self.inner.set_total(0, id.0)?;

        Ok(DbGroup {
            id,
            streams: self.streams.clone(),
            messages: self.messages.clone(),
            inner: self.inner.clone(),
        })
    }

    pub fn core(&self) -> DbCore {
        self.inner.clone()
    }
}

pub struct DbGroup {
    id: ConnectionId,
    streams: Arc<AtomicU64>,
    messages: Arc<AtomicU64>,
    inner: DbCore,
}

impl DbGroup {
    pub fn add(&self, meta: StreamMeta, kind: StreamKind) -> Result<DbStream, DbError> {
        let id = StreamId(self.streams.fetch_add(1, SeqCst));
        let connection_id = self.id;
        let v = Stream {
            connection_id,
            meta,
            kind,
        };
        self.inner.put_stream(id, v)?;
        self.inner.set_total(1, id.0)?;

        Ok(DbStream {
            id,
            messages: self.messages.clone(),
            inner: self.inner.clone(),
        })
    }
}

pub struct DbStream {
    id: StreamId,
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
        let offset = sb.lock().expect("poisoned").write(bytes)?;

        let id = MessageId(self.messages.fetch_add(1, SeqCst));
        let v = Message {
            stream_id: self.id,
            incoming,
            timestamp,
            offset,
            size: bytes.len() as u32,
        };
        self.inner.put_message(id, v)?;
        self.inner.set_total(2, id.0)?;

        Ok(())
    }
}
