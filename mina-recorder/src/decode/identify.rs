use serde::Serialize;

use prost::{bytes::Bytes, Message};

use crate::database::StreamKind;

use super::{DecodeError, utils};

#[allow(clippy::derive_partial_eq_without_eq)]
mod pb {
    include!(concat!(env!("OUT_DIR"), "/identify.proto.rs"));
}

pub fn parse(
    bytes: Vec<u8>,
    preview: bool,
    stream_kind: StreamKind,
) -> Result<serde_json::Value, DecodeError> {
    #[derive(Serialize)]
    struct T {
        protocol_version: Option<String>,
        agent_version: Option<String>,
        public_key: Option<String>,
        listen_addrs: Vec<String>,
        observed_addr: Option<String>,
        protocols: Vec<String>,
    }

    if preview {
        if matches!(stream_kind, StreamKind::IpfsId) {
            Ok(serde_json::Value::String("identify".to_string()))
        } else {
            Ok(serde_json::Value::String("identify_push".to_string()))
        }
    } else {
        let buf = Bytes::from(bytes);
        let pb::Identify {
            protocol_version,
            agent_version,
            public_key,
            listen_addrs,
            observed_addr,
            protocols,
        } = pb::Identify::decode_length_delimited(buf).map_err(DecodeError::Protobuf)?;

        let t = T {
            protocol_version,
            agent_version,
            public_key: public_key.map(hex::encode),
            listen_addrs: listen_addrs
                .into_iter()
                .map(|v| utils::parse_addr(&v))
                .collect(),
            observed_addr: observed_addr.map(|v| utils::parse_addr(&v)),
            protocols,
        };
        serde_json::to_value(&t).map_err(DecodeError::Serde)
    }
}

#[cfg(test)]
#[test]
fn decode_identify() {
    let hex = include_str!("identify.hex");
    let bytes = hex::decode(hex).unwrap();
    let msg = parse(bytes, false, StreamKind::IpfsId).unwrap();
    dbg!(msg);
}
