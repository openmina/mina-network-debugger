use std::{
    path::{PathBuf, Path},
    time::{Duration, SystemTime},
    cmp::Ordering,
    sync::{Mutex, Arc},
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{self, Write},
    os::unix::prelude::FileExt,
    convert::TryInto,
    net::SocketAddr,
};

use mina_p2p_messages::gossip::GossipNetMessageV2;
use radiation::{AbsorbExt, nom, ParseError, Emit};

use serde::Serialize;
use thiserror::Error;

use super::{
    types::{
        Connection, ConnectionId, StreamFullId, Message, StreamKind, FullMessage, MessageId,
        Timestamp, StatsDbKey,
    },
    params::{ValidParams, Coordinate, StreamFilter, Direction, KindFilter, ValidParamsConnection},
    index::{
        ConnectionIdx, StreamIdx, StreamByKindIdx, MessageKindIdx, AddressIdx, LedgerHash,
        LedgerHashIdx,
    },
    sorted_intersect::sorted_intersect,
};

use crate::{
    decode::{
        DecodeError, MessageType,
        meshsub_stats::{BlockStat, TxStat},
    },
    strace::StraceLine,
    meshsub::{SnarkByHash, Event, SnarkWithHash},
};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("{_0}")]
    CreateDirError(io::Error),
    #[error("{_0} {_1}")]
    Io(StreamFullId, io::Error),
    #[error("{_0} {_1}")]
    IoCn(ConnectionId, io::Error),
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
    #[error("param deserialize error {_0}")]
    ParamDeserialize(#[from] serde_json::Error),
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
        self.file.flush()?;
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
    raw_streams: Arc<Mutex<BTreeMap<ConnectionId, StreamBytesSync>>>,
    inner: Arc<rocksdb::DB>,
}

impl DbCore {
    const CFS: [&'static str; 12] = [
        Self::CONNECTIONS,
        Self::MESSAGES,
        Self::RANDOMNESS,
        Self::STRACE,
        Self::STATS,
        Self::STATS_TX,
        Self::CONNECTION_ID_INDEX,
        Self::STREAM_ID_INDEX,
        Self::STREAM_KIND_INDEX,
        Self::MESSAGE_KIND_INDEX,
        Self::ADDR_INDEX,
        Self::LEDGER_HASH_INDEX,
    ];

    const TTL: Duration = Duration::from_secs(0);

    const CONNECTIONS: &'static str = "connections";

    pub const CONNECTIONS_CNT: u8 = 0;

    const MESSAGES: &'static str = "messages";

    pub const MESSAGES_CNT: u8 = 1;

    const RANDOMNESS: &'static str = "randomness";

    pub const RANDOMNESS_CNT: u8 = 2;

    const STRACE: &'static str = "strace";

    pub const STRACE_CNT: u8 = 3;

    const STATS: &'static str = "stats";

    const STATS_TX: &'static str = "stats_tx";

    // indexes

    const CONNECTION_ID_INDEX: &'static str = "connection_id_index";

    const STREAM_ID_INDEX: &'static str = "stream_id_index";

    const STREAM_KIND_INDEX: &'static str = "stream_kind_index";

    const MESSAGE_KIND_INDEX: &'static str = "message_kind_index";

    const ADDR_INDEX: &'static str = "addr_index";

    const LEDGER_HASH_INDEX: &'static str = "ledger_hash_index";

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
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[2], Default::default()),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[3], Default::default()),
            // STATS
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[4], opts_with_prefix_extractor(4)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[5], Default::default()),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[6], opts_with_prefix_extractor(8)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[7], opts_with_prefix_extractor(16)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[8], opts_with_prefix_extractor(2)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[9], opts_with_prefix_extractor(2)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[10], opts_with_prefix_extractor(18)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[11], opts_with_prefix_extractor(32)),
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
            raw_streams: Default::default(),
            inner: Arc::new(inner),
        })
    }

    fn connections(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::CONNECTIONS).expect("must exist")
    }

    fn messages(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::MESSAGES).expect("must exist")
    }

    fn randomness(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::RANDOMNESS).expect("must exist")
    }

    fn strace(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::STRACE).expect("must exist")
    }

    fn stats(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::STATS).expect("must exist")
    }

    fn stats_tx(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::STATS_TX).expect("must exist")
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

    fn ledger_hash_index(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .cf_handle(Self::LEDGER_HASH_INDEX)
            .expect("must exist")
    }

    pub fn put_cn(&self, id: ConnectionId, v: Connection) -> Result<(), DbError> {
        self.inner
            .put_cf(self.connections(), id.chain(vec![]), v.chain(vec![]))?;

        Ok(())
    }

    pub fn put_message(
        &self,
        addr: &SocketAddr,
        id: MessageId,
        v: Message,
        tys: Vec<MessageType>,
        ledger_hashes: Vec<LedgerHash>,
    ) -> Result<(), DbError> {
        self.inner
            .put_cf(self.messages(), id.0.to_be_bytes(), v.chain(vec![]))?;
        let index = AddressIdx { addr: *addr, id };
        self.inner
            .put_cf(self.addr_index(), index.chain(vec![]), vec![])?;
        let index = ConnectionIdx {
            connection_id: v.connection_id,
            id,
        };
        self.inner
            .put_cf(self.connection_id_index(), index.chain(vec![]), vec![])?;
        let index = StreamIdx {
            stream_full_id: StreamFullId {
                cn: v.connection_id,
                id: v.stream_id,
            },
            id,
        };
        self.inner
            .put_cf(self.stream_id_index(), index.chain(vec![]), vec![])?;
        let index = StreamByKindIdx {
            stream_kind: v.stream_kind,
            id,
        };
        self.inner
            .put_cf(self.stream_kind_index(), index.chain(vec![]), vec![])?;
        for ty in tys {
            if matches!(&ty, &MessageType::HandshakePayload) {
                // peer id index
            }

            let index = MessageKindIdx { ty, id };
            self.inner
                .put_cf(self.message_kind_index(), index.chain(vec![]), vec![])?;
        }
        for hash in ledger_hashes {
            let message_id = id;
            let index = LedgerHashIdx {
                hash,
                offset: v.offset,
                size: v.size as u64,
                id: StreamFullId {
                    cn: v.connection_id,
                    id: v.stream_id,
                },
                message_id,
            };
            self.inner
                .put_cf(self.ledger_hash_index(), index.chain(vec![]), vec![])?;
        }
        Ok(())
    }

    pub fn put_randomness(&self, id: u64, bytes: Vec<u8>) -> Result<(), DbError> {
        self.inner
            .put_cf(self.randomness(), id.to_be_bytes(), bytes)?;

        Ok(())
    }

    pub fn put_strace(&self, id: u64, bytes: Vec<u8>) -> Result<(), DbError> {
        self.inner.put_cf(self.strace(), id.to_be_bytes(), bytes)?;

        Ok(())
    }

    pub fn put_stats(
        &self,
        height: u32,
        node_address: SocketAddr,
        bytes: Vec<u8>,
    ) -> Result<(), DbError> {
        let key = StatsDbKey {
            height,
            node_address,
        };

        self.inner.put_cf(self.stats(), key.chain(vec![]), bytes)?;

        Ok(())
    }

    pub fn put_stats_tx(&self, height: u32, bytes: Vec<u8>) -> Result<(), DbError> {
        self.inner
            .put_cf(self.stats_tx(), height.to_be_bytes(), bytes)?;

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn decode<K, T>(item: Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>) -> Option<(K, T)>
    where
        K: for<'pa> AbsorbExt<'pa> + std::fmt::Display,
        T: for<'pa> AbsorbExt<'pa>,
    {
        match item {
            Ok((key, value)) => match (K::absorb_ext(&key), T::absorb_ext(&value)) {
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
        T: for<'pa> AbsorbExt<'pa> + Timestamp,
    {
        let timestamp = Duration::from_secs(timestamp);
        if total == 0 {
            return Err(DbError::NoItemAtCursor("".to_string()));
        }
        let mut pos = total / 2;
        let mut r = pos;
        while r > 0 {
            let v = self.get::<T, _>(cf, pos.to_be_bytes())?;

            r /= 2;
            match v.timestamp().cmp(&timestamp) {
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
        Ok(self.inner.put([K], v.chain(vec![]))?)
    }

    pub fn fetch_connection(&self, id: u64) -> Result<Connection, DbError> {
        self.get(self.connections(), id.to_be_bytes())
    }

    pub fn get_raw_stream(&self, cn: ConnectionId) -> Result<Arc<Mutex<StreamBytes>>, DbError> {
        let mut lock = self.raw_streams.lock().expect("poisoned");
        let sb = lock.get(&cn).cloned();
        match sb {
            None => {
                let path = self.path.join("streams").join(cn.to_string());
                let sb = StreamBytes::new(path).map_err(|err| DbError::IoCn(cn, err))?;
                lock.insert(cn, sb.clone());
                drop(lock);
                Ok(sb)
            }
            Some(sb) => Ok(sb),
        }
    }

    pub fn remove_raw_stream(&self, cn: ConnectionId) {
        self.raw_streams.lock().expect("poisoned").remove(&cn);
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
        let r = self.get::<Connection, _>(self.connections(), msg.connection_id.0.to_be_bytes());
        let connection = match r {
            Ok(v) => v,
            Err(err) => {
                log::error!("{err}");
                return None;
            }
        };

        Some((
            key,
            FullMessage {
                connection_id: msg.connection_id,
                remote_addr: connection.info.addr,
                incoming: msg.incoming,
                timestamp: msg.timestamp,
                stream_id: msg.stream_id,
                stream_kind: msg.stream_kind,
                message: serde_json::Value::String(msg.brief),
                size: msg.size,
            },
        ))
    }

    // TODO: preview is useless
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
            // TODO: proper decode
            StreamKind::IpfsDelta => serde_json::Value::String(hex::encode(&buf)),
            StreamKind::PeerExchange => crate::decode::json_string::parse(buf, preview)?,
            // TODO: proper decode
            StreamKind::BitswapExchange => serde_json::Value::String(hex::encode(&buf)),
            // TODO: proper decode
            StreamKind::NodeStatus => serde_json::Value::String(hex::encode(&buf)),
            StreamKind::Select => {
                let s = String::from_utf8(buf)
                    .map_err(|err| DbError::Decode(DecodeError::Utf8(err)))?;
                serde_json::Value::String(s)
            }
            StreamKind::Mplex => {
                let v = buf.as_slice().try_into().map_err(|_| {
                    DbError::Decode(DecodeError::UnexpectedSize {
                        actual: buf.len(),
                        expected: 8,
                    })
                })?;
                let v = u64::from_be_bytes(v);
                let stream = v >> 3;
                let header = v & 7;
                let action = match header {
                    0 => "create stream",
                    3 => "close receiver",
                    4 => "close initiator",
                    5 => "reset receiver",
                    6 => "reset initiator",
                    1 | 2 | 7 => panic!("unexpected header {header}"),
                    _ => unreachable!(),
                };

                #[derive(Serialize)]
                struct MplexMessage {
                    action: &'static str,
                    stream: u64,
                }

                let msg = MplexMessage { action, stream };

                serde_json::to_value(&msg)
                    .map_err(|err| DbError::Decode(DecodeError::Serde(err)))?
            }
            StreamKind::Yamux => crate::decode::yamux::parse(buf, preview)?,
            StreamKind::Unknown => serde_json::Value::String(hex::encode(&buf)),
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

    fn connection_id(&self, params: &ValidParamsConnection) -> (bool, u64) {
        match params.coordinate.start {
            Coordinate::ById { id, explicit } => (explicit, id),
            Coordinate::ByTimestamp(timestamp) => {
                let total = self.total::<{ Self::CONNECTIONS_CNT }>().unwrap_or(0);
                match self.search_timestamp::<Connection>(self.connections(), total, timestamp) {
                    Ok(c) => (true, c),
                    Err(err) => {
                        log::error!("cannot find timestamp {timestamp}, err: {err}");
                        (false, 0)
                    }
                }
            }
        }
    }

    fn message_id(&self, params: &ValidParams) -> (bool, u64) {
        match params.coordinate.start {
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

    pub fn fetch_connections(
        &self,
        params: &ValidParamsConnection,
    ) -> impl Iterator<Item = (u64, serde_json::Value)> + '_ {
        let (present, id) = self.connection_id(params);

        let coordinate = &params.coordinate;
        let direction = coordinate.direction;

        let id = id.to_be_bytes();
        let mode = if present {
            rocksdb::IteratorMode::From(&id, direction.into())
        } else {
            direction.into()
        };

        let it = self
            .inner
            .iterator_cf(self.connections(), mode)
            .filter_map(Self::decode);
        let it = Box::new(it) as Box<dyn Iterator<Item = (u64, Connection)>>;
        let now = SystemTime::now();
        params.limit(it.filter_map(move |(id, cn)| {
            if cn.stats_in.total_bytes == 0 && cn.stats_out.total_bytes == 0 {
                return None;
            }
            Some((id, cn.post_process(Some(now))))
        }))
    }

    pub fn fetch_messages(
        &self,
        params: &ValidParams,
    ) -> impl Iterator<Item = (u64, FullMessage)> + '_ {
        let (present, id) = self.message_id(params);

        let coordinate = &params.coordinate;
        let direction = coordinate.direction;

        let it = if params.stream_filter.is_some() || params.kind_filter.is_some() {
            let stream_indexes = match &params.stream_filter {
                Some(StreamFilter::AnyStreamByAddr(addr)) => {
                    // TODO: duplicated code
                    let addr = *addr;
                    let id = AddressIdx {
                        addr,
                        id: MessageId(id),
                    };
                    let id = id.chain(vec![]);
                    let mode = rocksdb::IteratorMode::From(&id, direction.into());

                    let it = self
                        .inner
                        .iterator_cf(self.addr_index(), mode)
                        .filter_map(Self::decode_index::<AddressIdx>)
                        .take_while(move |index| index.addr == addr)
                        .map(|AddressIdx { id, .. }| id);
                    Some(Box::new(it) as Box<dyn Iterator<Item = MessageId>>)
                }
                Some(StreamFilter::AnyStreamInConnection(connection_id)) => {
                    let connection_id = *connection_id;
                    let id = ConnectionIdx {
                        connection_id,
                        id: MessageId(id),
                    };
                    let id = id.chain(vec![]);
                    let mode = rocksdb::IteratorMode::From(&id, direction.into());

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
                    let id = id.chain(vec![]);
                    let mode = rocksdb::IteratorMode::From(&id, direction.into());

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
                        let id = id.chain(vec![]);
                        let mode = rocksdb::IteratorMode::From(&id, direction.into());

                        self.inner
                            .iterator_cf(self.stream_kind_index(), mode)
                            .filter_map(Self::decode_index::<StreamByKindIdx>)
                            .take_while(move |index| index.stream_kind == stream_kind)
                            .map(|StreamByKindIdx { id, .. }| id)
                    });

                    let reverse = matches!(direction, Direction::Reverse);
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
                        let id = id.chain(vec![]);
                        let mode = rocksdb::IteratorMode::From(&id, direction.into());

                        let message_kind = message_kind.clone();
                        self.inner
                            .iterator_cf(self.message_kind_index(), mode)
                            .filter_map(Self::decode_index::<MessageKindIdx>)
                            .take_while(move |index| index.ty == message_kind.clone())
                            .map(|MessageKindIdx { id, .. }| id)
                    });

                    let reverse = matches!(direction, Direction::Reverse);
                    let predicate = move |a: &MessageId, b: &MessageId| (*a < *b) ^ reverse;
                    let it = itertools::kmerge_by(its, predicate);

                    Some(Box::new(it) as Box<dyn Iterator<Item = MessageId>>)
                }
                None => None,
            };
            match (stream_indexes, kind_indexes) {
                (Some(a), Some(b)) => {
                    let forward = matches!(&direction, &Direction::Forward);
                    let it = sorted_intersect(&mut [a, b], coordinate.limit, forward).into_iter();
                    self.fetch_messages_by_indexes(it)
                }
                (Some(i), None) => self.fetch_messages_by_indexes(i),
                (None, Some(i)) => self.fetch_messages_by_indexes(i),
                (None, None) => unreachable!(),
            }
        } else {
            let id = id.to_be_bytes();
            let mode = if present {
                rocksdb::IteratorMode::From(&id, direction.into())
            } else {
                direction.into()
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

    pub fn fetch_full_message_bin(&self, id: u64) -> Result<Vec<u8>, DbError> {
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
        Ok(buf)
    }

    pub fn fetch_full_message_hex(&self, id: u64) -> Result<String, DbError> {
        let buf = self.fetch_full_message_bin(id)?;
        Ok(hex::encode(&buf))
    }

    pub fn fetch_strace(
        &self,
        id: u64,
        timestamp: u64,
    ) -> Result<impl Iterator<Item = (u64, StraceLine)> + '_, DbError> {
        use rocksdb::{IteratorMode, Direction};

        let id = if timestamp == 0 {
            id
        } else {
            let total = self.total::<{ Self::STRACE_CNT }>().unwrap_or(0);
            self.search_timestamp::<StraceLine>(self.strace(), total, timestamp)?
        };

        let id = id.to_be_bytes();
        let it = self
            .inner
            .iterator_cf(self.strace(), IteratorMode::From(&id, Direction::Forward))
            .filter_map(Self::decode);
        Ok(it)
    }

    pub fn fetch_last_stat(&self) -> Option<(StatsDbKey, BlockStat)> {
        use rocksdb::IteratorMode;

        let (k, _) = self
            .inner
            .iterator_cf(self.stats(), IteratorMode::End)
            .next()
            .and_then(Self::decode::<StatsDbKey, BlockStat>)?;
        self.fetch_stats(k.height)
    }

    pub fn fetch_stats(&self, id: u32) -> Option<(StatsDbKey, BlockStat)> {
        let id_bytes = id.to_be_bytes();
        let mode = rocksdb::IteratorMode::From(&id_bytes, rocksdb::Direction::Forward);
        self.inner
            .iterator_cf(self.stats(), mode)
            .filter_map(Self::decode::<StatsDbKey, BlockStat>)
            .take_while(|(key, _)| key.height == id)
            .fold(None, |mut acc, (k, mut v)| {
                let (_, current) = acc.get_or_insert_with(|| {
                    let mut v = BlockStat::default();
                    v.height = k.height;
                    (k, v)
                });
                current.events.append(&mut v.events);
                acc
            })
    }

    pub fn fetch_last_stat_tx(&self) -> Option<(u32, TxStat)> {
        use rocksdb::IteratorMode;

        self.inner
            .iterator_cf(self.stats_tx(), IteratorMode::End)
            .next()
            .and_then(Self::decode)
    }

    pub fn fetch_stats_tx(&self, id: u32) -> Result<Option<(u32, TxStat)>, DbError> {
        match self.inner.get_cf(self.stats_tx(), id.to_be_bytes())? {
            None => Ok(None),
            Some(v) => Ok(Some((id, AbsorbExt::absorb_ext(&v)?))),
        }
    }

    pub fn fetch_snark_by_hash(&self, hash_str: String) -> Result<SnarkByHash, DbError> {
        let hash = serde_json::Value::String(hash_str.clone());
        let h = serde_json::from_value::<mina_p2p_messages::v2::LedgerHash>(hash)?;
        let o = |key_b: Vec<u8>| -> Result<Vec<(SnarkWithHash, u64)>, DbError> {
            let mut v = vec![];
            let mut deduplicate = HashSet::new();
            let key = rocksdb::IteratorMode::From(&key_b, rocksdb::Direction::Forward);
            let indexes = self
                .inner
                .iterator_cf(self.ledger_hash_index(), key)
                .filter_map(Self::decode_index::<LedgerHashIdx>)
                .take_while(|idx| idx.get_31().eq(&key_b[1..32]));
            for id in indexes {
                let mut buf = vec![0; id.size as usize];
                let sb = self.get_stream(id.id)?;
                let mut file = sb.lock().expect("poisoned");
                file.read(id.offset, &mut buf)
                    .map_err(|err| DbError::Io(id.id, err))?;
                drop(file);
                self.remove_stream(id.id);
                for event in crate::decode::meshsub::parse_it(&buf, false, true)? {
                    if let Event::PublishV2 { message, hash, .. } = event {
                        use self::SnarkWithHash::*;
                        match &*message {
                            GossipNetMessageV2::SnarkPoolDiff(snark) => {
                                let snark = match SnarkWithHash::try_from_inner(snark) {
                                    Some(v) => v,
                                    None => continue,
                                };

                                let conform = match (&snark, &id.hash) {
                                    (Leaf { hashes, .. }, LedgerHash::Source(v)) => {
                                        hashes[0].clone().into_inner().0.as_ref()[1..].eq(v)
                                    }
                                    (Leaf { hashes, .. }, LedgerHash::Target(v)) => {
                                        hashes[1].clone().into_inner().0.as_ref()[1..].eq(v)
                                    }
                                    (Merge { hashes, .. }, LedgerHash::FirstSource(v)) => {
                                        hashes[0].clone().into_inner().0.as_ref()[1..].eq(v)
                                    }
                                    (Merge { hashes, .. }, LedgerHash::Middle(v)) => {
                                        hashes[1].clone().into_inner().0.as_ref()[1..].eq(v)
                                    }
                                    (Merge { hashes, .. }, LedgerHash::SecondTarget(v)) => {
                                        hashes[2].clone().into_inner().0.as_ref()[1..].eq(v)
                                    }
                                    _ => false,
                                };
                                if conform {
                                    if deduplicate.insert(hash) {
                                        v.push((snark, id.message_id.0));
                                    }
                                }
                            }
                            GossipNetMessageV2::NewState(block) => {
                                for snark in SnarkWithHash::try_from_block(block) {
                                    let conform = match (&snark, &id.hash) {
                                        (Leaf { hashes, .. }, LedgerHash::Source(v)) => {
                                            hashes[0].clone().into_inner().0.as_ref()[1..].eq(v)
                                        }
                                        (Leaf { hashes, .. }, LedgerHash::Target(v)) => {
                                            hashes[1].clone().into_inner().0.as_ref()[1..].eq(v)
                                        }
                                        (Merge { hashes, .. }, LedgerHash::FirstSource(v)) => {
                                            hashes[0].clone().into_inner().0.as_ref()[1..].eq(v)
                                        }
                                        (Merge { hashes, .. }, LedgerHash::Middle(v)) => {
                                            hashes[1].clone().into_inner().0.as_ref()[1..].eq(v)
                                        }
                                        (Merge { hashes, .. }, LedgerHash::SecondTarget(v)) => {
                                            hashes[2].clone().into_inner().0.as_ref()[1..].eq(v)
                                        }
                                        _ => false,
                                    };
                                    if conform {
                                        if deduplicate.insert(hash) {
                                            v.push((snark, id.message_id.0));
                                        }
                                    }
                                }
                            }
                            _ => (),
                        }
                    }
                }
            }
            Ok(v)
        };
        Ok(SnarkByHash {
            source: o(LedgerHashIdx::source(h.clone()).chain(vec![]))?,
            target: o(LedgerHashIdx::target(h.clone()).chain(vec![]))?,
            first_source: o(LedgerHashIdx::first_source(h.clone()).chain(vec![]))?,
            middle: o(LedgerHashIdx::middle(h.clone()).chain(vec![]))?,
            second_target: o(LedgerHashIdx::second_target(h).chain(vec![]))?,
        })
    }
}

pub trait RandomnessDatabase {
    fn iterate_randomness<'a>(&'a self) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a>;
}

impl RandomnessDatabase for DbCore {
    fn iterate_randomness<'a>(&'a self) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a> {
        let it = self
            .inner
            .iterator_cf(self.randomness(), rocksdb::IteratorMode::End)
            .filter_map(Result::ok)
            .map(|(_, v)| v);
        Box::new(it)
    }
}
