use std::{
    path::{PathBuf, Path},
    time::{Duration, SystemTime},
    cmp::Ordering,
    sync::{Arc, Mutex},
    collections::{BTreeMap, HashSet, BTreeSet},
    io,
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
        Timestamp, StatsDbKey, StatsV2DbKey, CapnpEventWithMetadata, CapnpEventWithMetadataKey,
        CapnpTableRow, CapnpEventDecoded,
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
        meshsub_stats::{self, BlockStat, TxStat, Hash},
    },
    strace::StraceLine,
    meshsub::{SnarkByHash, Event, SnarkWithHash},
    ChunkHeader,
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

#[derive(Clone)]
pub struct DbCore {
    cache: Arc<Mutex<BTreeMap<ConnectionId, u64>>>,
    inner: Arc<rocksdb::DB>,
}

impl DbCore {
    const CFS: [&'static str; 16] = [
        Self::CONNECTIONS,
        Self::MESSAGES,
        Self::RANDOMNESS,
        Self::STRACE,
        Self::STATS,
        Self::STATS_TX,
        Self::CAPNP,
        Self::STATS_BLOCK_V2,
        Self::BLOBS,
        Self::KEYS,
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

    const CAPNP: &'static str = "capnp_data";

    const STATS_BLOCK_V2: &'static str = "stats_block_v2";

    const BLOBS: &'static str = "blobs";

    const KEYS: &'static str = "keys";

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
            // CAPNP
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[6], Default::default()),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[7], opts_with_prefix_extractor(4)),
            // BLOBS
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[8], Default::default()),
            // KEYS
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[9], Default::default()),
            // INDEXES
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[10], opts_with_prefix_extractor(8)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[11], opts_with_prefix_extractor(16)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[12], opts_with_prefix_extractor(2)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[13], opts_with_prefix_extractor(2)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[14], opts_with_prefix_extractor(18)),
            rocksdb::ColumnFamilyDescriptor::new(Self::CFS[15], opts_with_prefix_extractor(32)),
        ];
        let inner =
            rocksdb::DB::open_cf_descriptors_with_ttl(&opts, path.join("rocksdb"), cfs, Self::TTL)?;

        Ok(DbCore {
            cache: Arc::new(Mutex::new(BTreeMap::default())),
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

    fn stats_block_v2(&self) -> &rocksdb::ColumnFamily {
        self.inner
            .cf_handle(Self::STATS_BLOCK_V2)
            .expect("must exist")
    }

    fn stats_tx(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::STATS_TX).expect("must exist")
    }

    fn capnp(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::CAPNP).expect("must exist")
    }

    // TODO: store raw stream blobs in db, remove `raw_streams`
    #[allow(dead_code)]
    fn blobs(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::BLOBS).expect("must exist")
    }

    fn keys(&self) -> &rocksdb::ColumnFamily {
        self.inner.cf_handle(Self::KEYS).expect("must exist")
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

    pub fn put_randomness(&self, id: u64, bytes: [u8; 32]) -> Result<(), DbError> {
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

    pub fn put_stats_block_v2(&self, event: meshsub_stats::Event) -> Result<(), DbError> {
        let key = StatsV2DbKey {
            height: event.block_height,
            time: event.better_time,
        };

        self.inner.put_cf(
            self.stats_block_v2(),
            key.chain(vec![]),
            event.chain(vec![]),
        )?;

        Ok(())
    }

    pub fn put_stats_tx(&self, height: u32, bytes: Vec<u8>) -> Result<(), DbError> {
        self.inner
            .put_cf(self.stats_tx(), height.to_be_bytes(), bytes)?;

        Ok(())
    }

    pub fn put_capnp(
        &self,
        key: CapnpEventWithMetadataKey,
        event: CapnpEventWithMetadata,
    ) -> Result<(), DbError> {
        self.inner
            .put_cf(self.capnp(), key.chain(vec![]), event.chain(vec![]))?;

        Ok(())
    }

    pub fn put_blob(&self, cn: ConnectionId, data: &[u8]) -> Result<u64, DbError> {
        let mut lock = self.cache.lock().expect("must be ok");
        let position = lock.entry(cn).or_default();
        if *position == 0 {
            let key = (cn, u64::MAX).chain(vec![]);
            let mode = rocksdb::IteratorMode::From(&key, rocksdb::Direction::Reverse);
            let offset = match self.inner.iterator_cf(self.blobs(), mode).next() {
                None => 0,
                Some(r) => {
                    let (key, _) = r?;
                    let (cn_last, offset) = <(ConnectionId, u64)>::absorb_ext(&key)?;
                    if cn_last == cn {
                        offset + 1
                    } else {
                        0
                    }
                }
            };
            *position = offset;
        }
        let offset = *position;
        *position = offset + data.len() as u64;
        drop(lock);

        let key = (cn, offset).chain(vec![]);
        self.inner.put_cf(self.blobs(), key, data)?;

        Ok(offset)
    }

    pub fn fetch_blob(&self, cn: ConnectionId, offset: u64) -> Result<Vec<u8>, DbError> {
        let key = (cn, offset).chain(vec![]);
        let data = self
            .inner
            .get_cf(self.blobs(), key)?
            .ok_or(DbError::NoItemAtCursor(format!("{cn}, offset: {offset}")))?;
        Ok(data[ChunkHeader::SIZE..].to_vec())
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
        let connection =
            self.get::<Connection, _>(self.connections(), msg.connection_id.0.to_be_bytes())?;
        let buf = self.fetch_blob(msg.connection_id, msg.offset)?;
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

        self.fetch_blob(msg.connection_id, msg.offset)
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

    pub fn fetch_last_stat_block_v2(&self) -> Option<(u32, Vec<meshsub_stats::Event>)> {
        use rocksdb::IteratorMode;

        self.inner
            .iterator_cf(self.stats_block_v2(), IteratorMode::End)
            .next()
            .and_then(Self::decode::<StatsV2DbKey, meshsub_stats::Event>)
            .map(|(k, _)| (k.height, self.fetch_stats_block_v2(k.height)))
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

    pub fn fetch_stats_block_v2(&self, id: u32) -> Vec<meshsub_stats::Event> {
        let id_bytes = id.to_be_bytes();
        let mode = rocksdb::IteratorMode::From(&id_bytes, rocksdb::Direction::Forward);
        self.inner
            .iterator_cf(self.stats_block_v2(), mode)
            .filter_map(Self::decode::<StatsV2DbKey, meshsub_stats::Event>)
            .take_while(|(key, _)| key.height == id)
            .map(|(_, v)| v)
            .collect()
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
                let buf = self.fetch_blob(id.id.cn, id.offset)?;
                for event in crate::decode::meshsub::parse_it(&buf, false, true)? {
                    if let Event::PublishV2 { message, hash, .. } = event {
                        use self::SnarkWithHash::*;
                        match &*message {
                            GossipNetMessageV2::SnarkPoolDiff { message, .. } => {
                                let snark = match SnarkWithHash::try_from_inner(message) {
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

    pub fn fetch_capnp_latest(
        &self,
        all: bool,
    ) -> Option<impl Iterator<Item = CapnpTableRow> + '_> {
        let (k, _) = self
            .inner
            .iterator_cf(self.capnp(), rocksdb::IteratorMode::End)
            .next()
            .and_then(Self::decode::<CapnpEventWithMetadataKey, CapnpEventWithMetadata>)?;
        Some(self.fetch_capnp(k.height, all))
    }

    pub fn fetch_capnp_all(&self) -> impl Iterator<Item = CapnpTableRow> + '_ {
        self.inner
            .iterator_cf(self.capnp(), rocksdb::IteratorMode::Start)
            .filter_map(Self::decode::<CapnpEventWithMetadataKey, CapnpEventWithMetadata>)
            .map(|(k, v)| CapnpTableRow::transform(k, v))
    }

    pub fn fetch_capnp(&self, height: u32, all: bool) -> impl Iterator<Item = CapnpTableRow> + '_ {
        type State = BTreeMap<SocketAddr, (BTreeSet<Hash>, BTreeSet<Hash>)>;

        let key = height.to_be_bytes();
        self.inner
            .iterator_cf(
                self.capnp(),
                rocksdb::IteratorMode::From(&key, rocksdb::Direction::Forward),
            )
            .filter_map(Self::decode::<CapnpEventWithMetadataKey, CapnpEventWithMetadata>)
            .take_while(move |(k, _)| k.height == height)
            .map(|(k, v)| CapnpTableRow::transform(k, v))
            .scan(State::default(), move |state, mut v| {
                if all {
                    Some(v)
                } else {
                    let (sent, received) = state.entry(v.node_address).or_default();
                    v.events.retain(|x| match x {
                        CapnpEventDecoded::PublishGossip { hash, .. } => sent.insert(*hash),
                        CapnpEventDecoded::ReceivedGossip { hash, .. } => received.insert(*hash),
                    });
                    if v.events.is_empty() {
                        None
                    } else {
                        Some(v)
                    }
                }
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

pub trait KeyDatabase {
    fn reproduced_sk<const EPHEMERAL: bool>(&self, pk: [u8; 32]) -> Option<[u8; 32]>;
}

impl KeyDatabase for DbCore {
    fn reproduced_sk<const EPHEMERAL: bool>(&self, pk: [u8; 32]) -> Option<[u8; 32]> {
        use sha3::{
            Shake256,
            digest::{Update, ExtendableOutput, XofReader},
        };

        // try lookup the key
        self.inner
            .get_cf(self.keys(), pk)
            .ok()?
            .and_then(|x| x.try_into().ok())
            .or_else(|| {
                // If the key is not found in cache, reconstruct it.
                // Already have this number of keys
                let count = self
                    .inner
                    .iterator_cf(self.keys(), rocksdb::IteratorMode::Start)
                    .count();
                // The seed
                self.iterate_randomness()
                    .take(4)
                    .filter_map(|x| <[u8; 32]>::try_from(x.to_vec()).ok())
                    .find_map(|seed| {
                        log::debug!("try seed {}, ephemeral: {EPHEMERAL}", hex::encode(&seed));
                        let mut generator = Shake256::default()
                            .chain(seed)
                            .chain(if EPHEMERAL {
                                b"ephemeral".as_ref()
                            } else {
                                b"static".as_ref()
                            })
                            .finalize_xof();

                        use curve25519_dalek::{
                            montgomery::MontgomeryPoint, constants::ED25519_BASEPOINT_TABLE,
                            scalar::Scalar,
                        };
                        let point = MontgomeryPoint(pk);
                        // search further
                        (0..(count + 1))
                            .find_map(|_| {
                                let _ = (point, &mut generator);
                                let mut sk_bytes = [0; 32];
                                generator.read(&mut sk_bytes);
                                log::debug!("sk bytes: {}", hex::encode(sk_bytes));
                                sk_bytes[0] &= 248;
                                sk_bytes[31] |= 64;
                                let sk = Scalar::from_bits(sk_bytes);

                                if (&ED25519_BASEPOINT_TABLE * &sk).to_montgomery().eq(&point) {
                                    Some(sk_bytes)
                                } else {
                                    None
                                }
                            })
                            .inspect(|sk| {
                                self.inner.put_cf(self.keys(), pk, sk).unwrap_or_default();
                            })
                    })
            })
    }
}

#[cfg(test)]
#[test]
fn duplicates_removed() {
    use crate::libp2p_helper::CapnpEvent;

    let b0 = include_bytes!(
        "../test_data/block_1a57e382e918e0cde7cdd7493cf9b6b755299a785c1b97ddc2bc1cf66e91e647"
    );
    let h0 = hex::decode("1a57e382e918e0cde7cdd7493cf9b6b755299a785c1b97ddc2bc1cf66e91e647")
        .unwrap()
        .try_into()
        .unwrap();
    let b1 = include_bytes!(
        "../test_data/block_03d1a805254741ed5ad8b056e64b121f465323041d1f41d9df3db58b87670460"
    );
    let h1 = hex::decode("03d1a805254741ed5ad8b056e64b121f465323041d1f41d9df3db58b87670460")
        .unwrap()
        .try_into()
        .unwrap();

    std::fs::remove_dir_all("/tmp/test_duplicates_removed").unwrap_or_default();
    let db = DbCore::open("/tmp/test_duplicates_removed").unwrap();
    let node_address = "0.0.0.0:0".parse().unwrap();

    // put only b0
    let time = SystemTime::now();
    let key = CapnpEventWithMetadataKey { height: 5, time };
    let value = CapnpEventWithMetadata {
        real_time: time,
        node_address,
        events: vec![CapnpEvent::ReceivedGossip {
            peer_id: String::new(),
            peer_host: "0.1.2.3".to_string(),
            peer_port: 1,
            msg: b0[8..].to_vec(),
            hash: h0,
        }],
    };
    db.put_capnp(key, value).unwrap();

    // put single b0 and two b1
    let time = time + Duration::from_secs(1);
    let key = CapnpEventWithMetadataKey { height: 5, time };
    let value = CapnpEventWithMetadata {
        real_time: time,
        node_address,
        events: vec![
            CapnpEvent::ReceivedGossip {
                peer_id: String::new(),
                peer_host: "0.1.2.4".to_string(),
                peer_port: 1,
                msg: b0[8..].to_vec(),
                hash: h0,
            },
            CapnpEvent::ReceivedGossip {
                peer_id: String::new(),
                peer_host: "0.1.2.5".to_string(),
                peer_port: 1,
                msg: b1[8..].to_vec(),
                hash: h1,
            },
            CapnpEvent::ReceivedGossip {
                peer_id: String::new(),
                peer_host: "0.1.2.6".to_string(),
                peer_port: 1,
                msg: b1[8..].to_vec(),
                hash: h1,
            },
        ],
    };
    db.put_capnp(key, value).unwrap();

    // put only b0, but for different node, check it is not filtered out
    let time = time + Duration::from_secs(2);
    let key = CapnpEventWithMetadataKey { height: 5, time };
    let value = CapnpEventWithMetadata {
        real_time: time,
        node_address: "0.0.0.0:1".parse().unwrap(),
        events: vec![CapnpEvent::ReceivedGossip {
            peer_id: String::new(),
            peer_host: "0.1.2.4".to_string(),
            peer_port: 1,
            msg: b0[8..].to_vec(),
            hash: h0,
        }],
    };
    db.put_capnp(key, value).unwrap();

    // put only b0, check empty array is eliminated
    let time = time + Duration::from_secs(3);
    let key = CapnpEventWithMetadataKey { height: 5, time };
    let value = CapnpEventWithMetadata {
        real_time: time,
        node_address,
        events: vec![CapnpEvent::ReceivedGossip {
            peer_id: String::new(),
            peer_host: "0.1.2.4".to_string(),
            peer_port: 1,
            msg: b0[8..].to_vec(),
            hash: h0,
        }],
    };
    db.put_capnp(key, value).unwrap();

    db.inner.flush().unwrap();

    // fetch all
    let mut result = db.fetch_capnp(5, true);
    assert_eq!(result.next().unwrap().events.len(), 1);
    assert_eq!(result.next().unwrap().events.len(), 3);
    assert_eq!(result.next().unwrap().events.len(), 1);
    assert_eq!(result.next().unwrap().events.len(), 1);

    // fetch deduplicated
    let mut result = db.fetch_capnp(5, false);
    assert_eq!(result.next().unwrap().events.len(), 1);
    assert_eq!(result.next().unwrap().events.len(), 1);
    assert_eq!(result.next().unwrap().events.len(), 1);
    assert!(result.next().is_none());
}
