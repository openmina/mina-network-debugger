use std::{
    path::{PathBuf, Path},
    time::{Duration, SystemTime},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, Arc,
    },
    collections::BTreeMap,
    fs::File,
    io::{Write, self},
};

use radiation::{AbsorbExt, nom, ParseError, Emit};

use crate::ConnectionInfo;

use super::types::{
    Connection, ConnectionId, Stream, StreamId, Message, MessageId, StreamMeta, StreamKind,
};

pub enum DbError {
    Io(io::Error),
    Inner(rocksdb::Error),
    Parse(nom::Err<ParseError<Vec<u8>>>),
}

impl From<io::Error> for DbError {
    fn from(v: io::Error) -> Self {
        DbError::Io(v)
    }
}

impl From<rocksdb::Error> for DbError {
    fn from(v: rocksdb::Error) -> Self {
        DbError::Inner(v)
    }
}

impl<'pa> From<nom::Err<ParseError<&'pa [u8]>>> for DbError {
    fn from(v: nom::Err<ParseError<&'pa [u8]>>) -> Self {
        DbError::Parse(v.map(|e| e.into_vec()))
    }
}

struct StreamBytes {
    offset: u64,
    file: File,
}

impl StreamBytes {
    fn new<P>(path: P) -> io::Result<StreamBytesSync>
    where
        P: AsRef<Path>,
    {
        Ok(Arc::new(Mutex::new(StreamBytes {
            offset: 0,
            file: File::create(path)?,
        })))
    }
}

type StreamBytesSync = Arc<Mutex<StreamBytes>>;

#[derive(Clone)]
struct DbMsg {
    messages: Arc<AtomicU64>,
    path: PathBuf,
    stream_bytes: Arc<Mutex<BTreeMap<StreamId, StreamBytesSync>>>,
    inner: Arc<rocksdb::DB>,
}

pub struct DbFacade {
    cns: AtomicU64,
    streams: Arc<AtomicU64>,
    inner: DbMsg,
}

impl DbFacade {
    const CFS: [&'static str; 3] = [Self::CONNECTIONS, Self::STREAMS, Self::MESSAGES];

    const CONNECTIONS: &'static str = "connections";

    const STREAMS: &'static str = "streams";

    const MESSAGES: &'static str = "messages";

    const TTL: Duration = Duration::from_secs(120);

    fn connections(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .inner
            .cf_handle(Self::CONNECTIONS)
            .expect("must exist")
    }

    pub fn open<P>(path: P) -> Result<Self, DbError>
    where
        P: AsRef<Path>,
    {
        let path = PathBuf::from(path.as_ref());

        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        let inner =
            rocksdb::DB::open_cf_with_ttl(&opts, path.join("rocksdb"), Self::CFS, Self::TTL)?;
        let get_atomic_counter = |k| -> Result<AtomicU64, DbError> {
            match inner.get([k])? {
                None => Ok(AtomicU64::default()),
                Some(b) => Ok(AtomicU64::new(u64::absorb_ext(&b)?)),
            }
        };

        Ok(DbFacade {
            cns: get_atomic_counter(0)?,
            streams: Arc::new(get_atomic_counter(1)?),
            inner: DbMsg {
                messages: Arc::new(get_atomic_counter(2)?),
                path,
                stream_bytes: Arc::default(),
                inner: Arc::new(inner),
            },
        })
    }

    pub fn add(
        &self,
        info: ConnectionInfo,
        incoming: bool,
        timestamp: SystemTime,
    ) -> Result<DbGroup, DbError> {
        let id = ConnectionId(self.cns.fetch_add(1, Ordering::SeqCst));
        let v = Connection {
            info,
            incoming,
            timestamp,
        };
        self.inner
            .inner
            .put_cf(self.connections(), id.emit(vec![]), v.emit(vec![]))?;

        Ok(DbGroup {
            id,
            streams: self.streams.clone(),
            inner: self.inner.clone(),
        })
    }
}

pub struct DbGroup {
    id: ConnectionId,
    streams: Arc<AtomicU64>,
    inner: DbMsg,
}

impl DbGroup {
    fn streams(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .inner
            .cf_handle(DbFacade::STREAMS)
            .expect("must exist")
    }

    pub fn add(&self, meta: StreamMeta, kind: StreamKind) -> Result<DbStream, DbError> {
        let id = StreamId(self.streams.fetch_add(1, Ordering::SeqCst));
        let connection_id = self.id;
        let v = Stream {
            connection_id,
            meta,
            kind,
        };
        self.inner
            .inner
            .put_cf(self.streams(), id.emit(vec![]), v.emit(vec![]))?;

        Ok(DbStream {
            id,
            inner: self.inner.clone(),
        })
    }
}

pub struct DbStream {
    id: StreamId,
    inner: DbMsg,
}

impl DbStream {
    fn messages(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .inner
            .cf_handle(DbFacade::MESSAGES)
            .expect("must exist")
    }

    pub fn add(&self, incoming: bool, timestamp: SystemTime, bytes: &[u8]) -> Result<(), DbError> {
        let mut lock = self.inner.stream_bytes.lock().expect("poisoned");
        let stream_id = self.id;
        let sb = lock.get(&stream_id).cloned();
        let sb = match sb {
            None => {
                let path = self.inner.path.join("streams").join(stream_id.to_string());
                let sb = StreamBytes::new(path)?;
                lock.insert(stream_id, sb.clone());
                sb
            }
            Some(sb) => sb,
        };
        drop(lock);

        let mut lock = sb.lock().expect("poisoned");
        let offset = lock.offset;
        lock.file.write_all(bytes)?;
        lock.offset += bytes.len() as u64;
        drop(lock);

        let id = MessageId(self.inner.messages.fetch_add(1, Ordering::SeqCst));
        let v = Message {
            stream_id,
            incoming,
            timestamp,
            offset,
            size: bytes.len() as u32,
        };
        self.inner
            .inner
            .put_cf(self.messages(), id.emit(vec![]), v.emit(vec![]))?;

        Ok(())
    }
}
