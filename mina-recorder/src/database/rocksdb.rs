use std::{
    path::{PathBuf, Path},
    time::{Duration, SystemTime},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, Arc,
    },
    collections::BTreeMap,
    fs::{File, self},
    io::{Write, self},
    os::unix::prelude::FileExt,
};

use radiation::{AbsorbExt, nom, ParseError, Emit};

use thiserror::Error;

use crate::ConnectionInfo;

use super::types::{
    Connection, ConnectionId, Stream, StreamId, Message, MessageId, StreamMeta, StreamKind,
    FullMessage,
};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("{_0}")]
    Io(io::Error),
    #[error("{_0}")]
    Inner(rocksdb::Error),
    #[error("{_0}")]
    Parse(nom::Err<ParseError<Vec<u8>>>),
    #[error("no such stream {_0}")]
    NoSuchStream(StreamId),
    #[error("no such connection {_0}")]
    NoSuchConnection(ConnectionId),
    #[error("read beyond stream {stream_id} end {border} > {size}")]
    BeyondStreamEnd {
        stream_id: StreamId,
        border: u64,
        size: u64,
    },
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
        let file = File::options()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;
        let offset = file.metadata()?.len();
        Ok(Arc::new(Mutex::new(StreamBytes { offset, file })))
    }
}

type StreamBytesSync = Arc<Mutex<StreamBytes>>;

#[derive(Clone)]
pub struct DbCore {
    path: PathBuf,
    stream_bytes: Arc<Mutex<BTreeMap<StreamId, StreamBytesSync>>>,
    inner: Arc<rocksdb::DB>,
}

impl DbCore {
    fn connections(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .cf_handle(DbFacade::CONNECTIONS)
            .expect("must exist")
    }

    fn streams(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(DbFacade::STREAMS).expect("must exist")
    }

    fn messages(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .cf_handle(DbFacade::MESSAGES)
            .expect("must exist")
    }

    #[allow(clippy::type_complexity)]
    fn decode<T>(item: Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>) -> Option<T>
    where
        T: for<'pa> AbsorbExt<'pa>,
    {
        match item {
            Ok((_, value)) => match T::absorb_ext(&value) {
                Ok(v) => Some(v),
                Err(err) => {
                    log::error!("{err}");
                    None
                }
            },
            Err(err) => {
                log::error!("{err}");
                None
            }
        }
    }

    pub fn fetch_connections(
        &self,
        filter: &(),
        cursor: u64,
    ) -> impl Iterator<Item = Connection> + '_ {
        let _ = (filter, cursor);
        self.inner
            .iterator_cf(self.connections(), rocksdb::IteratorMode::Start)
            .filter_map(Self::decode)
    }

    pub fn fetch_streams(&self, filter: &(), cursor: u64) -> impl Iterator<Item = Stream> + '_ {
        let _ = (filter, cursor);
        self.inner
            .iterator_cf(self.streams(), rocksdb::IteratorMode::Start)
            .filter_map(Self::decode)
    }

    pub fn read_stream(
        &self,
        stream_id: StreamId,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<(), DbError> {
        let mut lock = self.stream_bytes.lock().expect("poisoned");
        let sb = lock.get(&stream_id).cloned();
        let sb = match sb {
            None => {
                let path = self.path.join("streams").join(stream_id.to_string());
                let sb = StreamBytes::new(path)?;
                lock.insert(stream_id, sb.clone());
                sb
            }
            Some(sb) => sb,
        };

        drop(lock);

        let lock = sb.lock().expect("poisoned");
        let size = lock.offset;
        let border = offset + buf.len() as u64;
        if border > size {
            return Err(DbError::BeyondStreamEnd {
                stream_id,
                border,
                size,
            });
        }
        lock.file.read_exact_at(buf, offset).unwrap();
        Ok(())
    }

    pub fn fetch_messages(&self, filter: &(), cursor: u64) -> impl Iterator<Item = Message> + '_ {
        let _ = (filter, cursor);
        let cursor = cursor.to_be_bytes();
        let mode = rocksdb::IteratorMode::From(&cursor, rocksdb::Direction::Forward);
        self.inner
            .iterator_cf(self.messages(), mode)
            .filter_map(Self::decode)
    }

    pub fn fetch_details(&self, msg: Message) -> Result<FullMessage, DbError> {
        let v = self
            .inner
            .get_cf(self.streams(), msg.stream_id.emit(vec![]))?
            .ok_or(DbError::NoSuchStream(msg.stream_id))?;
        let Stream {
            connection_id,
            meta,
            kind,
        } = AbsorbExt::absorb_ext(&v)?;
        let v = self
            .inner
            .get_cf(self.connections(), connection_id.emit(vec![]))?
            .ok_or(DbError::NoSuchConnection(connection_id))?;
        let Connection {
            info,
            incoming,
            timestamp,
        } = AbsorbExt::absorb_ext(&v)?;
        let mut buf = vec![0; msg.size as usize];
        self.read_stream(msg.stream_id, msg.offset, &mut buf)?;
        let message = match kind {
            StreamKind::Raw => serde_json::Value::String(hex::encode(&buf)),
            _ => serde_json::Value::String(hex::encode(&buf)),
        };
        Ok(FullMessage {
            cn_timestamp: timestamp,
            cn_info: info,
            cn_incoming: incoming,
            stream_meta: meta,
            stream_kind: kind,
            incoming: msg.incoming,
            timestamp: msg.timestamp,
            message,
        })
    }

    pub fn fetch_full(&self, filter: &(), cursor: u64) -> impl Iterator<Item = FullMessage> + '_ {
        self.fetch_messages(filter, cursor)
            .filter_map(|msg| match self.fetch_details(msg) {
                Ok(v) => {
                    if v.stream_kind == StreamKind::Handshake {
                        None
                    } else {
                        Some(v)
                    }
                }
                Err(err) => {
                    log::error!("{err}");
                    None
                }
            })
    }
}

pub struct DbFacade {
    cns: AtomicU64,
    streams: Arc<AtomicU64>,
    messages: Arc<AtomicU64>,
    inner: DbCore,
}

impl DbFacade {
    const CFS: [&'static str; 3] = [Self::CONNECTIONS, Self::STREAMS, Self::MESSAGES];

    const CONNECTIONS: &'static str = "connections";

    const STREAMS: &'static str = "streams";

    const MESSAGES: &'static str = "messages";

    const TTL: Duration = Duration::from_secs(120);

    pub fn open<P>(path: P) -> Result<Self, DbError>
    where
        P: AsRef<Path>,
    {
        let path = PathBuf::from(path.as_ref());

        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let inner =
            rocksdb::DB::open_cf_with_ttl(&opts, path.join("rocksdb"), Self::CFS, Self::TTL)?;
        let get_atomic_counter = |k| -> Result<AtomicU64, DbError> {
            match inner.get([k])? {
                None => Ok(AtomicU64::default()),
                Some(b) => Ok(AtomicU64::new(u64::absorb_ext(&b)?)),
            }
        };
        fs::create_dir(path.join("streams")).or_else(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(e)
            }
        })?;

        Ok(DbFacade {
            cns: get_atomic_counter(0)?,
            streams: Arc::new(get_atomic_counter(1)?),
            messages: Arc::new(get_atomic_counter(2)?),
            inner: DbCore {
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
            .put_cf(self.inner.connections(), id.emit(vec![]), v.emit(vec![]))?;

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
        let id = StreamId(self.streams.fetch_add(1, Ordering::SeqCst));
        let connection_id = self.id;
        let v = Stream {
            connection_id,
            meta,
            kind,
        };
        self.inner
            .inner
            .put_cf(self.inner.streams(), id.emit(vec![]), v.emit(vec![]))?;

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

impl DbStream {
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

        let id = MessageId(self.messages.fetch_add(1, Ordering::SeqCst));
        let v = Message {
            stream_id,
            incoming,
            timestamp,
            offset,
            size: bytes.len() as u32,
        };
        self.inner
            .inner
            .put_cf(self.inner.messages(), id.emit(vec![]), v.emit(vec![]))?;

        Ok(())
    }
}
