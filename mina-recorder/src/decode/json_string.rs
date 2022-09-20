use super::{DecodeError, MessageType};

pub fn parse(bytes: Vec<u8>, preview: bool) -> Result<serde_json::Value, DecodeError> {
    if preview {
        serde_json::to_value(MessageType::PeerExchange).map_err(DecodeError::Serde)
    } else {
        let s = String::from_utf8(bytes).map_err(DecodeError::Utf8)?;
        serde_json::from_str(&s).map_err(DecodeError::Serde)
    }
}
