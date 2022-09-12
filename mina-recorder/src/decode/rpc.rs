use std::io::{Cursor, Read};

use serde::Serialize;
use mina_p2p_messages::{
    rpc::{QueryHeader, JSONinifyPayloadReader, JSONinifyError},
    utils, JSONifyPayloadRegistry,
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
    enum Msg {
        Response {
            tag: String,
            version: i32,
            id: i64,
            value: serde_json::Value,
        },
        Request {
            tag: String,
            version: i32,
            id: i64,
            query: serde_json::Value,
        },
    }

    struct DefaultReader;

    impl JSONinifyPayloadReader for DefaultReader {
        fn read_query(&self, r: &mut dyn Read) -> Result<serde_json::Value, JSONinifyError> {
            let mut v = vec![];
            r.read_to_end(&mut v).map_err(From::from).map_err(JSONinifyError::Binprot)?;
            let t = serde_json::to_value(hex::encode(v))?;
            Ok(t)
        }

        fn read_response(&self, r: &mut dyn Read) -> Result<serde_json::Value, JSONinifyError> {
            self.read_query(r)
        }
    }

    let mut stream = Cursor::new(&bytes);

    let _len = utils::stream_decode_size(&mut stream)?;
    let Nat0(d) = BinProtRead::binprot_read(&mut stream)?;
    let msg = QueryHeader::binprot_read(&mut stream)?;
    let tag = String::from_utf8(msg.tag.as_ref().to_vec()).map_err(DecodeError::Utf8)?;
    let r = JSONifyPayloadRegistry::new();
    let reader = r.get(&tag, msg.version)
        .unwrap_or(&DefaultReader);
    match d {
        1 => {
            if preview {
                Ok(serde_json::Value::String(format!("Request {tag}")))
            } else {
                let query = reader.read_query(&mut stream)?;
                serde_json::to_value(Msg::Request {
                    tag,
                    version: msg.version,
                    id: msg.id,
                    query,
                })
                .map_err(DecodeError::Serde)
            }
        }
        2 => {
            if preview {
                Ok(serde_json::Value::String(format!("Response {tag}")))
            } else {
                let value = reader.read_response(&mut stream)?;
                serde_json::to_value(Msg::Response {
                    tag,
                    version: msg.version,
                    id: msg.id,
                    value,
                })
                .map_err(DecodeError::Serde)
            }
        }
        _ => Ok(serde_json::Value::Null),
    }
}
