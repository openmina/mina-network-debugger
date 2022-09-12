use std::io::Cursor;

use serde::Serialize;
use mina_p2p_messages::{
    rpc::{QueryHeader, RpcMethod, NeedsLength, RpcResult, Error as RpcError},
    utils, GetEpochLedger,
};
use binprot::{BinProtRead, Nat0};

use super::{DecodeError, MessageType};

pub fn parse_types(bytes: &[u8]) -> Result<Vec<MessageType>, DecodeError> {
    let mut stream = Cursor::new(&bytes);

    let _len = utils::stream_decode_size(&mut stream)?;
    let Nat0(_) = BinProtRead::binprot_read(&mut stream)?;
    let msg = QueryHeader::binprot_read(&mut stream)?;
    let tag = String::from_utf8(msg.tag.as_ref().to_vec()).map_err(DecodeError::Utf8)?;

    Ok(tag.parse().ok().into_iter().collect())
}

pub fn parse(bytes: Vec<u8>, preview: bool) -> Result<serde_json::Value, DecodeError> {
    #[derive(Serialize)]
    #[serde(rename_all = "snake_case")]
    #[serde(tag = "type")]
    enum Msg<Q, R> {
        Response {
            tag: String,
            version: i32,
            id: i64,
            value: Option<RpcResult<NeedsLength<R>, RpcError>>,
        },
        Request {
            tag: String,
            version: i32,
            id: i64,
            query: Option<NeedsLength<Q>>,
        },
    }

    pub struct Empty;

    impl RpcMethod for Empty {
        const NAME: &'static str = "empty";

        const VERSION: i32 = 0;

        type Query = ();

        type Response = ();
    }

    let mut stream = Cursor::new(&bytes);

    let _len = utils::stream_decode_size(&mut stream)?;
    let Nat0(d) = BinProtRead::binprot_read(&mut stream)?;
    let msg = QueryHeader::binprot_read(&mut stream)?;
    let tag = String::from_utf8(msg.tag.as_ref().to_vec()).map_err(DecodeError::Utf8)?;
    match d {
        1 => {
            if preview {
                Ok(serde_json::Value::String(format!("Request {tag}")))
            } else {
                match tag.as_str() {
                    "get_epoch_ledger" => {
                        type Q = <GetEpochLedger as RpcMethod>::Query;
                        type R = <GetEpochLedger as RpcMethod>::Response;

                        serde_json::to_value(Msg::Request::<Q, R> {
                            tag,
                            version: msg.version,
                            id: msg.id,
                            query: Some(BinProtRead::binprot_read(&mut stream)?),
                        })
                        .map_err(DecodeError::Serde)
                    }
                    _ => serde_json::to_value(Msg::Request::<(), ()> {
                        tag,
                        version: msg.version,
                        id: msg.id,
                        query: None,
                    })
                    .map_err(DecodeError::Serde),
                }
            }
        }
        2 => {
            if preview {
                Ok(serde_json::Value::String(format!("Response {tag}")))
            } else {
                match tag.as_str() {
                    "get_epoch_ledger" => {
                        type Q = <GetEpochLedger as RpcMethod>::Query;
                        type R = <GetEpochLedger as RpcMethod>::Response;

                        serde_json::to_value(Msg::Response::<Q, R> {
                            tag,
                            version: msg.version,
                            id: msg.id,
                            value: Some(BinProtRead::binprot_read(&mut stream)?),
                        })
                        .map_err(DecodeError::Serde)
                    }
                    _ => serde_json::to_value(Msg::Response::<(), ()> {
                        tag,
                        version: msg.version,
                        id: msg.id,
                        value: None,
                    })
                    .map_err(DecodeError::Serde),
                }
            }
        }
        _ => Ok(serde_json::Value::Null),
    }
}
