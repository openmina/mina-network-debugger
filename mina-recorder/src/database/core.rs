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

use super::types::{
    Connection, ConnectionId, StreamFullId, Message, StreamKind, FullMessage, MessageId,
};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("{_0}")]
    CreateDirError(io::Error),
    #[error("{_0} {_1}")]
    Io(StreamFullId, io::Error),
    #[error("{_0}")]
    Inner(rocksdb::Error),
    #[error("{_0}")]
    Parse(nom::Err<ParseError<Vec<u8>>>),
    #[error("no such stream {_0}")]
    NoSuchStream(StreamFullId),
    #[error("no such connection {_0}")]
    NoSuchConnection(ConnectionId),
    #[error("read beyond stream {stream_full_id} end {border} > {size}")]
    BeyondStreamEnd {
        stream_full_id: StreamFullId,
        border: u64,
        size: u64,
    },
    #[error("no item at id {_0}")]
    NoItemAtCursor(u64),
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

    pub fn read(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let size = self.offset;
        let border = offset + buf.len() as u64;
        if border > size {
            let msg = format!("read beyond end {border} > {size}");
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, msg));
        }
        self.file.read_exact_at(buf, offset)?;
        Ok(())
    }
}

type StreamBytesSync = Arc<Mutex<StreamBytes>>;

#[derive(Clone)]
pub struct DbCore {
    path: PathBuf,
    stream_bytes: Arc<Mutex<BTreeMap<StreamFullId, StreamBytesSync>>>,
    inner: Arc<rocksdb::DB>,
}

#[derive(Deserialize)]
pub struct Params {
    // the start of the list, either id of record ...
    id: Option<u64>,
    // ... or timestamp
    timestamp: Option<u64>,
    // wether go `forward` or `reverse`, default is `forward`
    #[serde(default = "default_direction")]
    direction: Direction,
    // how many records to read, default is 16
    #[serde(default = "default_limit")]
    limit: usize,
    // what streams to read, comma separated
    // streams: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Forward,
    Reverse,
}

impl From<Direction> for rocksdb::Direction {
    fn from(v: Direction) -> Self {
        match v {
            Direction::Forward => rocksdb::Direction::Forward,
            Direction::Reverse => rocksdb::Direction::Reverse,
        }
    }
}

const fn default_direction() -> Direction {
    Direction::Forward
}

const fn default_limit() -> usize {
    16
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
        fs::create_dir(path.join("streams"))
            .or_else(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(e)
                }
            })
            .map_err(DbError::CreateDirError)?;

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
        let direction = filter.direction.into();
        let mut id = filter.id.unwrap_or(0).to_be_bytes();
        let mode = match (direction, filter.timestamp, filter.id) {
            (rocksdb::Direction::Forward, None, None) => rocksdb::IteratorMode::Start,
            (rocksdb::Direction::Reverse, None, None) => rocksdb::IteratorMode::End,
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
            .take(filter.limit)
    }

    pub fn get_stream(
        &self,
        stream_full_id: StreamFullId,
    ) -> Result<Arc<Mutex<StreamBytes>>, DbError> {
        let mut lock = self.stream_bytes.lock().expect("poisoned");
        let sb = lock.get(&stream_full_id).cloned();
        match sb {
            None => {
                let path = self.path.join("streams").join(stream_full_id.to_string());
                let sb = StreamBytes::new(path).map_err(|err| DbError::Io(stream_full_id, err))?;
                lock.insert(stream_full_id, sb.clone());
                drop(lock);
                Ok(sb)
            }
            Some(sb) => Ok(sb),
        }
    }

    pub fn remove_stream(&self, stream_full_id: StreamFullId) {
        self.stream_bytes
            .lock()
            .expect("poisoned")
            .remove(&stream_full_id);
    }

    fn fetch_details(&self, msg: Message) -> Result<FullMessage, DbError> {
        let stream_full_id = StreamFullId {
            cn: msg.connection_id,
            id: msg.stream_id,
        };
        let mut buf = vec![0; msg.size as usize];
        let sb = self.get_stream(stream_full_id)?;
        let mut file = sb.lock().expect("poisoned");
        file.read(msg.offset, &mut buf)
            .map_err(|err| DbError::Io(stream_full_id, err))?;
        drop(file);
        let message = match msg.stream_kind {
            StreamKind::Kad => {
                let v = crate::connection::mina_protocol::kademlia::parse(buf);
                serde_json::to_value(&v).unwrap()
            }
            StreamKind::Meshsub => {
                let v = crate::connection::mina_protocol::meshsub::parse(buf);
                serde_json::to_value(&v).unwrap()
            }
            _ => serde_json::Value::String(hex::encode(&buf)),
        };
        Ok(FullMessage {
            connection_id: msg.connection_id,
            incoming: msg.incoming,
            timestamp: msg.timestamp,
            stream_id: msg.stream_id,
            stream_kind: msg.stream_kind,
            message,
        })
    }

    pub fn fetch_messages(&self, filter: &Params) -> impl Iterator<Item = (u64, FullMessage)> + '_ {
        let direction = filter.direction.into();
        let mut id = filter.id.unwrap_or(0).to_be_bytes();
        let mode = match (direction, filter.timestamp, filter.id) {
            (rocksdb::Direction::Forward, None, None) => rocksdb::IteratorMode::Start,
            (rocksdb::Direction::Reverse, None, None) => rocksdb::IteratorMode::End,
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
                Ok(v) => Some((key, v)),
                Err(err) => {
                    log::error!("{err}");
                    None
                }
            })
            .take(filter.limit)
    }
}
