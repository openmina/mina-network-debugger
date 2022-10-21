use super::DecodeError;

use super::yamux_parser::Header;

pub fn parse(bytes: Vec<u8>, _: bool) -> Result<serde_json::Value, DecodeError> {
    if bytes.len() == 12 {
        let header_bytes = <[u8; 12]>::try_from(bytes.as_slice()).expect("cannot fail");
        Header::try_from(header_bytes)
            .map_err(DecodeError::Yamux)
            .and_then(|header| serde_json::to_value(&header).map_err(DecodeError::Serde))
    } else {
        Err(DecodeError::UnexpectedSize { actual: bytes.len(), expected: 12 })
    }
}
