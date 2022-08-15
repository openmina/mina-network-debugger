use std::{
    path::{PathBuf, Path},
    time::{SystemTime, Duration},
    cmp::Ordering,
    sync::{Mutex, Arc},
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Write},
    os::unix::prelude::FileExt,
};

use serde::Deserialize;

use radiation::{AbsorbExt, nom, ParseError, Emit};

use thiserror::Error;

use super::types::{Connection, ConnectionId, StreamId, Message, StreamKind, FullMessage, MessageId};

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
    #[error("no item at id {_0}")]
    NoItemAtCursor(u64),
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

pub struct StreamBytes {
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

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<u64> {
        let offset = self.offset;
        self.file.write_all(bytes)?;
        self.offset += bytes.len() as u64;
        Ok(offset)
    }
}

type StreamBytesSync = Arc<Mutex<StreamBytes>>;

#[derive(Clone)]
pub struct DbCore {
    path: PathBuf,
    stream_bytes: Arc<Mutex<BTreeMap<StreamId, StreamBytesSync>>>,
    inner: Arc<rocksdb::DB>,
}

#[derive(Deserialize)]
pub struct Params {
    // the start of the list, either id of record ...
    id: Option<u64>,
    // ... or timestamp
    timestamp: Option<u64>,
    // wether go forward or backward
    reverse: Option<bool>,
    // how many records to read
    limit: Option<u64>,
    // what streams to read, comma separated
    // streams: Option<String>,
}

impl DbCore {
    const CFS: [&'static str; 2] = [Self::CONNECTIONS, Self::MESSAGES];

    const TTL: Duration = Duration::from_secs(120);

    const CONNECTIONS: &'static str = "connections";

    pub const CONNECTIONS_CNT: u8 = 0;

    const MESSAGES: &'static str = "messages";

    pub const MESSAGES_CNT: u8 = 1;

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
        fs::create_dir(path.join("streams")).or_else(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(e)
            }
        })?;

        Ok(DbCore {
            path,
            stream_bytes: Arc::default(),
            inner: Arc::new(inner),
        })
    }

    fn connections(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::CONNECTIONS).expect("must exist")
    }

    fn messages(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::MESSAGES).expect("must exist")
    }

    pub fn put_cn(&self, id: ConnectionId, v: Connection) -> Result<(), DbError> {
        self.inner
            .put_cf(self.connections(), id.emit(vec![]), v.emit(vec![]))?;
        Ok(())
    }

    pub fn put_message(&self, id: MessageId, v: Message) -> Result<(), DbError> {
        self.inner
            .put_cf(self.messages(), id.emit(vec![]), v.emit(vec![]))?;
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn decode<T>(item: Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>) -> Option<(u64, T)>
    where
        T: for<'pa> AbsorbExt<'pa>,
    {
        match item {
            Ok((key, value)) => match (u64::absorb_ext(&key), T::absorb_ext(&value)) {
                (Ok(key), Ok(v)) => Some((key, v)),
                (Ok(key), Err(err)) => {
                    log::error!("key {key}, err: {err}");
                    None
                }
                (Err(err), _) => {
                    log::error!("key is unknown, err: {err}");
                    None
                }
            },
            Err(err) => {
                log::error!("{err}");
                None
            }
        }
    }

    fn search_timestamp<T>(
        &self,
        cf: &rocksdb::ColumnFamily,
        total: u64,
        timestamp: u64,
    ) -> Result<u64, DbError>
    where
        T: for<'pa> AbsorbExt<'pa> + AsRef<SystemTime>,
    {
        if total == 0 {
            return Err(DbError::NoItemAtCursor(0));
        }
        let mut pos = total / 2;
        let mut r = pos;
        while r > 0 {
            let v = self
                .inner
                .get_cf(cf, pos.to_be_bytes())?
                .ok_or(DbError::NoItemAtCursor(pos))?;
            let v = T::absorb_ext(&v)?;
            let this_timestamp = v
                .as_ref()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("after unix epoch")
                .as_secs();

            r /= 2;
            match this_timestamp.cmp(&timestamp) {
                Ordering::Less => pos += r,
                Ordering::Equal => r = 0,
                Ordering::Greater => pos -= r,
            }
        }
        Ok(pos)
    }

    pub fn total<const K: u8>(&self) -> Result<u64, DbError> {
        match self.inner.get([K])? {
            None => Ok(0),
            Some(b) => Ok(u64::absorb_ext(&b)?),
        }
    }

    pub fn set_total<const K: u8>(&self, v: u64) -> Result<(), DbError> {
        Ok(self.inner.put([K], v.emit(vec![]))?)
    }

    pub fn fetch_connections(
        &self,
        filter: &Params,
    ) -> impl Iterator<Item = (u64, Connection)> + '_ {
        let reverse = filter.reverse.unwrap_or(false);
        let direction = if reverse {
            rocksdb::Direction::Reverse
        } else {
            rocksdb::Direction::Forward
        };
        let mut id = filter.id.unwrap_or(0).to_be_bytes();
        let mode = match (reverse, filter.timestamp, filter.id) {
            (false, None, None) => rocksdb::IteratorMode::Start,
            (true, None, None) => rocksdb::IteratorMode::End,
            (_, None, Some(_)) => rocksdb::IteratorMode::From(&id, direction),
            (_, Some(timestamp), _) => {
                let total = self.total::<{ Self::CONNECTIONS_CNT }>().unwrap_or(0);
                match self.search_timestamp::<Connection>(self.connections(), total, timestamp) {
                    Ok(c) => {
                        id = c.to_be_bytes();
                        rocksdb::IteratorMode::From(&id, direction)
                    }
                    Err(err) => {
                        log::error!("cannot find timestamp {timestamp}, err: {err}");
                        rocksdb::IteratorMode::From(&id, direction)
                    }
                }
            }
        };

        self.inner
            .iterator_cf(self.connections(), mode)
            .filter_map(Self::decode)
            .take(filter.limit.unwrap_or(16) as usize)
    }

    pub fn get_stream(&self, stream_id: StreamId) -> Result<Arc<Mutex<StreamBytes>>, DbError> {
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

        Ok(sb)
    }

    pub fn remove_stream(&self, stream_id: StreamId) {
        self.stream_bytes
            .lock()
            .expect("poisoned")
            .remove(&stream_id);
    }

    fn read_stream(&self, stream_id: StreamId, offset: u64, buf: &mut [u8]) -> Result<(), DbError> {
        let sb = self.get_stream(stream_id)?;
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

    fn fetch_details(&self, msg: Message) -> Result<FullMessage, DbError> {
        let stream_id = StreamId {
            cn: msg.connection_id,
            meta: msg.stream_meta,
        };
        let mut buf = vec![0; msg.size as usize];
        self.read_stream(stream_id, msg.offset, &mut buf)?;
        let message = match msg.stream_kind {
            StreamKind::Raw => serde_json::Value::String(hex::encode(&buf)),
            StreamKind::Kad => {
                let v = crate::connection::mina_protocol::kademlia::parse(buf);
                serde_json::to_value(&v).unwrap()
            }
            _ => serde_json::Value::String(hex::encode(&buf)),
        };
        Ok(FullMessage {
            connection_id: msg.connection_id,
            incoming: msg.incoming,
            timestamp: msg.timestamp,
            stream_meta: msg.stream_meta,
            stream_kind: msg.stream_kind,
            message,
        })
    }

    pub fn fetch_messages(&self, filter: &Params) -> impl Iterator<Item = (u64, FullMessage)> + '_ {
        let reverse = filter.reverse.unwrap_or(false);
        let direction = if reverse {
            rocksdb::Direction::Reverse
        } else {
            rocksdb::Direction::Forward
        };
        let mut id = filter.id.unwrap_or(0).to_be_bytes();
        let mode = match (reverse, filter.timestamp, filter.id) {
            (false, None, None) => rocksdb::IteratorMode::Start,
            (true, None, None) => rocksdb::IteratorMode::End,
            (_, None, Some(_)) => rocksdb::IteratorMode::From(&id, direction),
            (_, Some(timestamp), _) => {
                let total = self.total::<{ Self::MESSAGES_CNT }>().unwrap_or(0);
                match self.search_timestamp::<Message>(self.messages(), total, timestamp) {
                    Ok(c) => {
                        id = c.to_be_bytes();
                        rocksdb::IteratorMode::From(&id, direction)
                    }
                    Err(err) => {
                        log::error!("cannot find timestamp {timestamp}, err: {err}");
                        rocksdb::IteratorMode::From(&id, direction)
                    }
                }
            }
        };

        self.inner
            .iterator_cf(self.messages(), mode)
            .filter_map(Self::decode)
            .filter_map(|(key, msg)| match self.fetch_details(msg) {
                Ok(v) => {
                    if v.stream_kind == StreamKind::Handshake {
                        None
                    } else {
                        Some((key, v))
                    }
                }
                Err(err) => {
                    log::error!("{err}");
                    None
                }
            })
            .take(filter.limit.unwrap_or(16) as usize)
    }
}
