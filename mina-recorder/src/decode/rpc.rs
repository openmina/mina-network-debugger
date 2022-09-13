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
            r.read_to_end(&mut v)
                .map_err(From::from)
                .map_err(JSONinifyError::Binprot)?;
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
    let reader = r.get(&tag, msg.version).unwrap_or(&DefaultReader);
    match d {
        1 => {
            if preview {
                Ok(serde_json::Value::String(format!("Request {tag}")))
            } else {
                let query = dbg!(reader.read_query(&mut stream)?);
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

#[cfg(test)]
#[test]
fn decode_get_ancestry_request() {
    let hex = "fe30020101010101fd47b4020001012401010e0b010105010103010106010104010104010105010105010106010103010104010106012018a10a97aecd08d4943494ea214bf98267acf3581e742dcf9939b8d50a9600000101fc41b39b1029a33d0d01010101fd96fc03000101fee41b0101fd96fc030001010101011aa5e6f6ec01b303f7a1c83621028d64dff5518da8d773faf4a24081cf58e8040101fc41f3331038f5220d012e75f2f08ff5b8643bb02018aa2a58cfed1f06620172664995cf97361940100801a5decf819d08686345822a90aaa7c0c7e86c3e0b9637c4c16555ba494e55a73201a75f038a83f4c506dc7368ce048e594156eca3d81bff12557a647f6a58c399320101fea01201010101015f31eee4a818d4ea59795a55ebffcaeb6aff41bc0729c1363204ac2897ff7c2b0101fc41f3864f1110330d01a0d68c0346cd6a055f6ab3a7eaa4ea75599e08ffb995455d7b12e6ea5a23561b01ff565955b71cc13797263918d78be3fdea93b2ca9e37fcbd672faf71e655560f0160d068f13ce2ab1b3713e0d301cb24e71ed9cdcb1803e3c262c2c411cba9c80e0101fe300b010101d2b188371899fa0cae753fdc4b9273b8854e98ff098c0157a6a3d5a31b12eb16010101c06d41b18396020a3505350c5061bb55b4a157b36a9e4f398238775d1dbb4d250101015acc0cc04158899f5eeef2e79d9501012aeabe97dba2fe3c431084a54adbd21600000184d9e97ac198403ae121bba6530126e9b6f35e389dab2490158e4c170ae6ca39";
    let bytes = hex::decode(hex).unwrap();
    let mut stream = Cursor::new(bytes);

    let r = JSONifyPayloadRegistry::new();
    let reader = r.get("get_ancestry", 1).unwrap();
    let value = reader.read_query(&mut stream).unwrap();
    assert_ne!(dbg!(value), serde_json::Value::Null);
}

#[cfg(test)]
#[test]
fn decode_get_transition_chain_proof_request() {
    let hex = "210175d3948a0ea418815c59780501fb75a8448d8c1b63f22993c8e028167f502a28";
    let bytes = hex::decode(hex).unwrap();
    let mut stream = Cursor::new(bytes);

    let r = JSONifyPayloadRegistry::new();
    let reader = r.get("get_transition_chain_proof", 1).unwrap();
    let value = reader.read_query(&mut stream).unwrap();
    assert_ne!(dbg!(value), serde_json::Value::Null);
}
