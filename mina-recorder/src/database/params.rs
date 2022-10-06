use std::{net::SocketAddr, str::FromStr};

use serde::Deserialize;

use thiserror::Error;

use crate::decode::MessageType;

use super::types::{ConnectionId, StreamFullId, StreamKind, Timestamp};

#[derive(Debug, Error)]
pub enum ParamsValidateError {
    #[error("cannot use together id and timestamp, ambiguous start")]
    IdWithTimestamp,
    #[error("cannot parse socket addr {_0}")]
    ParseSocketAddr(<SocketAddr as FromStr>::Err),
    #[error("cannot filter by stream id without connection id")]
    StreamIdWithoutConnectionId,
    #[error("cannot parse {_0}")]
    ParseStreamId(String),
    #[error("cannot parse message kind")]
    ParseMessageKind,
}

pub struct ValidParams {
    pub start: Coordinate,
    pub limit: usize,
    limit_timestamp: Option<u64>,
    pub direction: Direction,
    pub stream_filter: Option<StreamFilter>,
    pub kind_filter: Option<KindFilter>,
}

pub enum Coordinate {
    ById { id: u64, explicit: bool },
    ByTimestamp(u64),
}

pub enum StreamFilter {
    AnyStreamByAddr(SocketAddr),
    AnyStreamInConnection(ConnectionId),
    Stream(StreamFullId),
}

pub enum KindFilter {
    AnyMessageInStream(Vec<StreamKind>),
    Message(Vec<MessageType>),
}

#[derive(Default, Deserialize)]
pub struct Params {
    // the start of the list, either id of record ...
    id: Option<u64>,
    // ... or timestamp
    timestamp: Option<u64>,
    // wether go `forward` or `reverse`, default is `forward`
    #[serde(default)]
    direction: Direction,
    // how many records to read, default is 1 for connections and 16 for messages
    // if `limit_timestamp` is specified, default limit is `usize::MAX`
    limit: Option<usize>,
    limit_timestamp: Option<u64>,
    // what streams to read, comma separated
    // streams: Option<String>,
    // filter by connection id
    connection_id: Option<u64>,
    addr: Option<String>,
    stream_id: Option<String>,
    stream_kind: Option<String>,
    message_kind: Option<String>,
}

#[derive(Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
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

impl<'a> From<Direction> for rocksdb::IteratorMode<'a> {
    fn from(v: Direction) -> Self {
        match v {
            Direction::Forward => rocksdb::IteratorMode::Start,
            Direction::Reverse => rocksdb::IteratorMode::End,
        }
    }
}

impl Params {
    #[allow(dead_code)]
    pub fn with_stream_kind(mut self, stream_kind: StreamKind) -> Self {
        self.stream_kind = Some(stream_kind.to_string());
        self
    }

    #[allow(dead_code)]
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn validate(self) -> Result<ValidParams, ParamsValidateError> {
        let start = match (self.id, self.timestamp) {
            (None, None) => match self.direction {
                Direction::Forward => Coordinate::ById {
                    id: 0,
                    explicit: false,
                },
                Direction::Reverse => Coordinate::ById {
                    id: u64::MAX,
                    explicit: false,
                },
            },
            (Some(id), None) => Coordinate::ById { id, explicit: true },
            (None, Some(timestamp)) => Coordinate::ByTimestamp(timestamp),
            (Some(_), Some(_)) => return Err(ParamsValidateError::IdWithTimestamp),
        };
        let limit = if self.limit_timestamp.is_some() {
            self.limit.unwrap_or(usize::MAX)
        } else {
            self.limit.unwrap_or(16)
        };
        let stream_filter = match (self.addr, self.connection_id, self.stream_id) {
            (Some(addr), _, _) => {
                let addr = addr.parse().map_err(ParamsValidateError::ParseSocketAddr)?;
                Some(StreamFilter::AnyStreamByAddr(addr))
            }
            (None, None, None) => None,
            (None, Some(id), None) => Some(StreamFilter::AnyStreamInConnection(ConnectionId(id))),
            (None, Some(id), Some(s)) => {
                let stream_id = s.parse().map_err(ParamsValidateError::ParseStreamId)?;
                Some(StreamFilter::Stream(StreamFullId {
                    cn: ConnectionId(id),
                    id: stream_id,
                }))
            }
            (None, None, Some(_)) => return Err(ParamsValidateError::StreamIdWithoutConnectionId),
        };
        let kind_filter = match (self.stream_kind, self.message_kind) {
            (None, None) => None,
            (Some(kind), None) => {
                let kinds = kind
                    .split(',')
                    .map(|s| s.parse().expect("cannot fail"))
                    .collect();
                Some(KindFilter::AnyMessageInStream(kinds))
            }
            (_, Some(kind)) => {
                let mut kinds = Vec::new();
                for s in kind.split(',') {
                    kinds.push(
                        s.parse()
                            .map_err(|()| ParamsValidateError::ParseMessageKind)?,
                    );
                }
                Some(KindFilter::Message(kinds))
            }
        };
        Ok(ValidParams {
            start,
            limit,
            limit_timestamp: self.limit_timestamp,
            direction: self.direction,
            stream_filter,
            kind_filter,
        })
    }
}

impl ValidParams {
    pub fn limit<'a, It, T>(&self, it: It) -> impl Iterator<Item = (u64, T)> + 'a
    where
        It: Iterator<Item = (u64, T)> + 'a,
        T: Timestamp,
    {
        let limit_timestamp = self.limit_timestamp;
        it.take_while(move |(_, msg)| {
            if let Some(limit_timestamp) = limit_timestamp {
                let d = msg.timestamp();
                d.as_secs() < limit_timestamp
            } else {
                true
            }
        })
        .take(self.limit)
    }
}
