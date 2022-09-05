use std::io::Cursor;

use serde::Serialize;
use mina_p2p_messages::{rpc::QueryHeader, utils};
use binprot::{BinProtRead, Nat0};

use super::{DecodeError, MessageType};

pub fn parse_types(bytes: &[u8]) -> Result<Vec<MessageType>, DecodeError> {
    let mut stream = Cursor::new(&bytes);

    let _len = utils::decode_size(&mut stream).map_err(DecodeError::BinProt)?;
    let Nat0(_) = BinProtRead::binprot_read(&mut stream).map_err(DecodeError::BinProt)?;
    let msg = QueryHeader::binprot_read(&mut stream).map_err(DecodeError::BinProt)?;
    let tag = String::from_utf8(msg.tag.as_ref().to_vec()).map_err(DecodeError::Utf8)?;

    Ok(vec![MessageType::Rpc { tag }])
}

pub fn parse(bytes: Vec<u8>, preview: bool) -> Result<serde_json::Value, DecodeError> {
    #[derive(Serialize)]
    #[serde(rename_all = "snake_case")]
    #[serde(tag = "type")]
    enum Msg {
        Response { tag: String, version: i32, id: i64 },
        Request { tag: String, version: i32, id: i64 },
    }

    let mut stream = Cursor::new(&bytes);

    let _len = utils::decode_size(&mut stream).map_err(DecodeError::BinProt)?;
    let Nat0(d) = BinProtRead::binprot_read(&mut stream).map_err(DecodeError::BinProt)?;
    let msg = QueryHeader::binprot_read(&mut stream).map_err(DecodeError::BinProt)?;
    let tag = String::from_utf8(msg.tag.as_ref().to_vec()).map_err(DecodeError::Utf8)?;
    match d {
        1 => {
            if preview {
                Ok(serde_json::Value::String(format!("Request {tag}")))
            } else {
                serde_json::to_value(Msg::Request {
                    tag,
                    version: msg.version,
                    id: msg.id,
                })
                .map_err(DecodeError::Serde)
            }
        }
        2 => {
            if preview {
                Ok(serde_json::Value::String(format!("Response {tag}")))
            } else {
                serde_json::to_value(Msg::Response {
                    tag,
                    version: msg.version,
                    id: msg.id,
                })
                .map_err(DecodeError::Serde)
            }
        }
        _ => Ok(serde_json::Value::Null),
    }
}
