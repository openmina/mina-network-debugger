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

use radiation::{AbsorbExt, nom, ParseError, Emit};

use thiserror::Error;

use super::{
    types::{Connection, ConnectionId, StreamFullId, Message, StreamKind, FullMessage, MessageId},
    params::{ValidParams, Coordinate, StreamFilter, Direction, KindFilter},
    index::{ConnectionIdx, StreamIdx, StreamByKindIdx, MessageKindIdx},
    sorted_intersect::sorted_intersect,
};

use crate::{
    decode::{DecodeError, MessageType},
    custom_coding,
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
    NoItemAtCursor(String),
    #[error("decode {_0}")]
    Decode(DecodeError),
}

impl From<DecodeError> for DbError {
    fn from(v: DecodeError) -> Self {
        DbError::Decode(v)
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

impl DbCore {
    const CFS: [&'static str; 7] = [
        Self::CONNECTIONS,
        Self::MESSAGES,
        Self::CONNECTION_ID_INDEX,
        Self::STREAM_ID_INDEX,
        Self::STREAM_KIND_INDEX,
        Self::MESSAGE_KIND_INDEX,
        Self::ADDR_INDEX,
    ];

    const TTL: Duration = Duration::from_secs(120);

    const CONNECTIONS: &'static str = "connections";

    pub const CONNECTIONS_CNT: u8 = 0;

    const MESSAGES: &'static str = "messages";

    pub const MESSAGES_CNT: u8 = 1;

    // indexes

    const CONNECTION_ID_INDEX: &'static str = "connection_id_index";

    const STREAM_ID_INDEX: &'static str = "stream_id_index";

    const STREAM_KIND_INDEX: &'static str = "stream_kind_index";

    const MESSAGE_KIND_INDEX: &'static str = "message_kind_index";

    const ADDR_INDEX: &'static str = "addr_index";

    pub fn open<P>(path: P) -> Result<Self, DbError>
    where
        P: AsRef<Path>,
    {
        let path = PathBuf::from(path.as_ref());

        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let opts_with_prefix_extractor = |prefix_len| {
            let mut opts = rocksdb::Options::default();
            opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(prefix_len));
            opts
        };
        let cfs = [
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[0], Default::default()),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[1], Default::default()),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[2], opts_with_prefix_extractor(8)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[3], opts_with_prefix_extractor(16)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[4], opts_with_prefix_extractor(2)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[5], opts_with_prefix_extractor(2)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[6], opts_with_prefix_extractor(18)),
        ];
        let inner =
            rocksdb::DB::open_cf_descriptors_with_ttl(&opts, path.join("rocksdb"), cfs, Self::TTL)?;
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

    fn connection_id_index(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .cf_handle(Self::CONNECTION_ID_INDEX)
            .expect("must exist")
    }

    fn stream_id_index(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .cf_handle(Self::STREAM_ID_INDEX)
            .expect("must exist")
    }

    fn stream_kind_index(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .cf_handle(Self::STREAM_KIND_INDEX)
            .expect("must exist")
    }

    fn message_kind_index(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .cf_handle(Self::MESSAGE_KIND_INDEX)
            .expect("must exist")
    }

    fn addr_index(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::ADDR_INDEX).expect("must exist")
    }

    pub fn put_cn(&self, id: ConnectionId, v: Connection) -> Result<(), DbError> {
        self.inner
            .put_cf(self.connections(), id.emit(vec![]), v.emit(vec![]))?;

        let key = custom_coding::addr_emit(&v.info.addr, vec![]);
        self.inner.put_cf(self.addr_index(), key, id.emit(vec![]))?;

        Ok(())
    }

    pub fn put_message(
        &self,
        id: MessageId,
        v: Message,
        tys: Vec<MessageType>,
    ) -> Result<(), DbError> {
        self.inner
            .put_cf(self.messages(), id.emit(vec![]), v.emit(vec![]))?;
        let index = ConnectionIdx {
            connection_id: v.connection_id,
            id,
        };
        self.inner
            .put_cf(self.connection_id_index(), index.emit(vec![]), vec![])?;
        let index = StreamIdx {
            stream_full_id: StreamFullId {
                cn: v.connection_id,
                id: v.stream_id,
            },
            id,
        };
        self.inner
            .put_cf(self.stream_id_index(), index.emit(vec![]), vec![])?;
        let index = StreamByKindIdx {
            stream_kind: v.stream_kind,
            id,
        };
        self.inner
            .put_cf(self.stream_kind_index(), index.emit(vec![]), vec![])?;
        for ty in tys {
            if matches!(&ty, &MessageType::HandshakePayload) {
                // peer id index
            }

            let index = MessageKindIdx { ty, id };
            self.inner
                .put_cf(self.message_kind_index(), index.emit(vec![]), vec![])?;
        }
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

    #[allow(dead_code)]
    #[allow(clippy::type_complexity)]
    fn decode_index<T>(item: Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>) -> Option<T>
    where
        T: for<'pa> AbsorbExt<'pa>,
    {
        match item {
            Ok((key, _)) => match T::absorb_ext(&key) {
                Ok(v) => Some(v),
                Err(err) => {
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

    fn get<T, K>(&self, cf: &rocksdb::ColumnFamily, key: K) -> Result<T, DbError>
    where
        K: AsRef<[u8]>,
        T: for<'pa> AbsorbExt<'pa>,
    {
        let v = self
            .inner
            .get_cf(cf, &key)?
            .ok_or_else(|| DbError::NoItemAtCursor(hex::encode(key.as_ref())))?;
        let v = T::absorb_ext(&v)?;
        Ok(v)
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
        let timestamp = Duration::from_secs(timestamp);
        if total == 0 {
            return Err(DbError::NoItemAtCursor("".to_string()));
        }
        let mut pos = total / 2;
        let mut r = pos;
        while r > 0 {
            let v = self.get::<T, _>(cf, pos.to_be_bytes())?;
            let this_timestamp = v
                .as_ref()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("after unix epoch");

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

    pub fn fetch_connection(&self, id: u64) -> Result<Connection, DbError> {
        self.get(self.connections(), id.to_be_bytes())
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

    fn fetch_details(&self, (key, msg): (u64, Message)) -> Option<(u64, FullMessage)> {
        match self.fetch_details_inner(msg, true) {
            Ok(v) => Some((key, v)),
            Err(err) => {
                log::error!("{err}");
                None
            }
        }
    }

    fn fetch_details_inner(&self, msg: Message, preview: bool) -> Result<FullMessage, DbError> {
        let stream_full_id = StreamFullId {
            cn: msg.connection_id,
            id: msg.stream_id,
        };
        let connection =
            self.get::<Connection, _>(self.connections(), msg.connection_id.0.to_be_bytes())?;
        let mut buf = vec![0; msg.size as usize];
        let sb = self.get_stream(stream_full_id)?;
        let mut file = sb.lock().expect("poisoned");
        file.read(msg.offset, &mut buf)
            .map_err(|err| DbError::Io(stream_full_id, err))?;
        drop(file);
        self.remove_stream(stream_full_id);
        let message = match msg.stream_kind {
            StreamKind::Kad => crate::decode::kademlia::parse(buf, preview)?,
            StreamKind::Meshsub => crate::decode::meshsub::parse(buf, preview)?,
            StreamKind::Handshake => crate::decode::noise::parse(buf, preview)?,
            StreamKind::Rpc => crate::decode::rpc::parse(buf, preview)?,
            StreamKind::IpfsId => crate::decode::identify::parse(buf, preview, msg.stream_kind)?,
            StreamKind::IpfsPush => crate::decode::identify::parse(buf, preview, msg.stream_kind)?,
            _ => serde_json::Value::String(hex::encode(&buf)),
        };
        Ok(FullMessage {
            connection_id: msg.connection_id,
            remote_addr: connection.info.addr,
            incoming: msg.incoming,
            timestamp: msg.timestamp,
            stream_id: msg.stream_id,
            stream_kind: msg.stream_kind,
            message,
            size: msg.size,
        })
    }

    fn message_id(&self, params: &ValidParams) -> (bool, u64) {
        match params.start {
            Coordinate::ById { id, explicit } => (explicit, id),
            Coordinate::ByTimestamp(timestamp) => {
                let total = self.total::<{ Self::MESSAGES_CNT }>().unwrap_or(0);
                match self.search_timestamp::<Message>(self.messages(), total, timestamp) {
                    Ok(c) => (true, c),
                    Err(err) => {
                        log::error!("cannot find timestamp {timestamp}, err: {err}");
                        (false, 0)
                    }
                }
            }
        }
    }

    fn fetch_messages_by_indexes<'a, It>(
        &'a self,
        it: It,
    ) -> Box<dyn Iterator<Item = (u64, Message)> + 'a>
    where
        It: Iterator<Item = MessageId> + 'a,
    {
        let it = it.filter_map(|id| match self.get(self.messages(), id.0.to_be_bytes()) {
            Ok(v) => Some((id.0, v)),
            Err(err) => {
                log::error!("{err}");
                None
            }
        });
        Box::new(it) as Box<dyn Iterator<Item = (u64, Message)>>
    }

    pub fn fetch_messages(
        &self,
        params: &ValidParams,
    ) -> impl Iterator<Item = (u64, FullMessage)> + '_ {
        let (present, id) = self.message_id(params);

        let it = if params.stream_filter.is_some() || params.kind_filter.is_some() {
            let stream_indexes = match &params.stream_filter {
                Some(StreamFilter::AnyStreamByAddr(addr)) => {
                    let connection_id =
                        match self.get(self.addr_index(), custom_coding::addr_emit(addr, vec![])) {
                            Ok(v) => v,
                            Err(err) => {
                                log::warn!("no connection {addr}, {err}");
                                ConnectionId(u64::MAX)
                            }
                        };
                    // TODO: duplicated code
                    let id = ConnectionIdx {
                        connection_id,
                        id: MessageId(id),
                    };
                    let id = id.emit(vec![]);
                    let mode = rocksdb::IteratorMode::From(&id, params.direction.into());

                    let it = self
                        .inner
                        .iterator_cf(self.connection_id_index(), mode)
                        .filter_map(Self::decode_index::<ConnectionIdx>)
                        .take_while(move |index| index.connection_id == connection_id)
                        .map(|ConnectionIdx { id, .. }| id);
                    Some(Box::new(it) as Box<dyn Iterator<Item = MessageId>>)
                }
                Some(StreamFilter::AnyStreamInConnection(connection_id)) => {
                    let connection_id = *connection_id;
                    let id = ConnectionIdx {
                        connection_id,
                        id: MessageId(id),
                    };
                    let id = id.emit(vec![]);
                    let mode = rocksdb::IteratorMode::From(&id, params.direction.into());

                    let it = self
                        .inner
                        .iterator_cf(self.connection_id_index(), mode)
                        .filter_map(Self::decode_index::<ConnectionIdx>)
                        .take_while(move |index| index.connection_id == connection_id)
                        .map(|ConnectionIdx { id, .. }| id);
                    Some(Box::new(it) as Box<dyn Iterator<Item = MessageId>>)
                }
                Some(StreamFilter::Stream(stream_full_id)) => {
                    let stream_full_id = *stream_full_id;
                    let id = StreamIdx {
                        stream_full_id,
                        id: MessageId(id),
                    };
                    let id = id.emit(vec![]);
                    let mode = rocksdb::IteratorMode::From(&id, params.direction.into());

                    let it = self
                        .inner
                        .iterator_cf(self.stream_id_index(), mode)
                        .filter_map(Self::decode_index::<StreamIdx>)
                        .take_while(move |index| index.stream_full_id == stream_full_id)
                        .map(|StreamIdx { id, .. }| id);
                    Some(Box::new(it) as Box<dyn Iterator<Item = MessageId>>)
                }
                None => None,
            };
            let kind_indexes = match &params.kind_filter {
                Some(KindFilter::AnyMessageInStream(kinds)) => {
                    let its = kinds.iter().map(|stream_kind| {
                        let stream_kind = *stream_kind;
                        let id = StreamByKindIdx {
                            stream_kind,
                            id: MessageId(id),
                        };
                        let id = id.emit(vec![]);
                        let mode = rocksdb::IteratorMode::From(&id, params.direction.into());

                        self.inner
                            .iterator_cf(self.stream_kind_index(), mode)
                            .filter_map(Self::decode_index::<StreamByKindIdx>)
                            .take_while(move |index| index.stream_kind == stream_kind)
                            .map(|StreamByKindIdx { id, .. }| id)
                    });

                    let reverse = matches!(params.direction, Direction::Reverse);
                    let predicate = move |a: &MessageId, b: &MessageId| (*a < *b) ^ reverse;
                    let it = itertools::kmerge_by(its, predicate);

                    Some(Box::new(it) as Box<dyn Iterator<Item = MessageId>>)
                }
                Some(KindFilter::Message(kinds)) => {
                    let its = kinds.iter().map(|message_kind| {
                        let id = MessageKindIdx {
                            ty: message_kind.clone(),
                            id: MessageId(id),
                        };
                        let id = id.emit(vec![]);
                        let mode = rocksdb::IteratorMode::From(&id, params.direction.into());

                        let message_kind = message_kind.clone();
                        self.inner
                            .iterator_cf(self.message_kind_index(), mode)
                            .filter_map(Self::decode_index::<MessageKindIdx>)
                            .take_while(move |index| index.ty == message_kind.clone())
                            .map(|MessageKindIdx { id, .. }| id)
                    });

                    let reverse = matches!(params.direction, Direction::Reverse);
                    let predicate = move |a: &MessageId, b: &MessageId| (*a < *b) ^ reverse;
                    let it = itertools::kmerge_by(its, predicate);

                    Some(Box::new(it) as Box<dyn Iterator<Item = MessageId>>)
                }
                None => None,
            };
            match (stream_indexes, kind_indexes) {
                (Some(a), Some(b)) => {
                    let forward = matches!(&params.direction, &Direction::Forward);
                    let it = sorted_intersect(&mut [a, b], params.limit, forward).into_iter();
                    self.fetch_messages_by_indexes(it)
                }
                (Some(i), None) => self.fetch_messages_by_indexes(i),
                (None, Some(i)) => self.fetch_messages_by_indexes(i),
                (None, None) => unreachable!(),
            }
        } else {
            let id = id.to_be_bytes();
            let mode = if present {
                rocksdb::IteratorMode::From(&id, params.direction.into())
            } else {
                params.direction.into()
            };

            let it = self
                .inner
                .iterator_cf(self.messages(), mode)
                .filter_map(Self::decode);
            Box::new(it) as Box<dyn Iterator<Item = (u64, Message)>>
        };
        params.limit(it.filter_map(|v| self.fetch_details(v)))
    }

    pub fn fetch_full_message(&self, id: u64) -> Result<FullMessage, DbError> {
        let msg = self.get::<Message, _>(self.messages(), id.to_be_bytes())?;
        self.fetch_details_inner(msg, false)
    }

    pub fn fetch_full_message_hex(&self, id: u64) -> Result<String, DbError> {
        let msg = self.get::<Message, _>(self.messages(), id.to_be_bytes())?;

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
        Ok(hex::encode(&buf))
    }
}
