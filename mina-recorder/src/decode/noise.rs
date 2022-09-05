use prost::{bytes::Bytes, Message};
use serde::Serialize;

use super::{DecodeError, MessageType};

#[allow(clippy::derive_partial_eq_without_eq)]
mod pb {
    include!(concat!(env!("OUT_DIR"), "/envelope_proto.rs"));
}
#[allow(clippy::derive_partial_eq_without_eq)]
mod keys_proto {
    include!(concat!(env!("OUT_DIR"), "/keys_proto.rs"));
}

pub fn parse_types(_: &[u8]) -> Result<Vec<MessageType>, DecodeError> {
    Ok(vec![MessageType::HandshakePayload])
}

pub fn parse(bytes: Vec<u8>, _: bool) -> Result<serde_json::Value, DecodeError> {
    #[derive(Serialize)]
    struct T {
        r#type: String,
        public_key: String,
        peer_id: String,
        payload_type: String,
        payload: String,
        signature: String,
    }

    let buf = Bytes::from(bytes);
    let msg = pb::Envelope::decode(buf).map_err(DecodeError::Protobuf)?;

    let (r#type, public_key, peer_id) = msg
        .public_key
        .map(|pk| {
            let libp2p_pk = match pk.r#type() {
                keys_proto::KeyType::Rsa => {
                    let pk = libp2p_core::identity::rsa::PublicKey::decode_x509(&pk.data).unwrap();
                    libp2p_core::PublicKey::Rsa(pk)
                }
                keys_proto::KeyType::Ed25519 => {
                    let pk = libp2p_core::identity::ed25519::PublicKey::decode(&pk.data).unwrap();
                    libp2p_core::PublicKey::Ed25519(pk)
                }
                keys_proto::KeyType::Secp256k1 => {
                    let pk = libp2p_core::identity::secp256k1::PublicKey::decode(&pk.data).unwrap();
                    libp2p_core::PublicKey::Secp256k1(pk)
                }
                keys_proto::KeyType::Ecdsa => {
                    let pk = libp2p_core::identity::ecdsa::PublicKey::from_bytes(&pk.data).unwrap();
                    libp2p_core::PublicKey::Ecdsa(pk)
                }
            };
            let id = libp2p_core::PeerId::from_public_key(&libp2p_pk);
            (
                pk.r#type().as_str_name().to_string(),
                hex::encode(pk.data),
                id.to_base58(),
            )
        })
        .unwrap_or(("".to_string(), "".to_string(), "".to_string()));

    let t = T {
        r#type,
        public_key,
        peer_id,
        payload_type: hex::encode(msg.payload_type),
        payload: hex::encode(msg.payload),
        signature: hex::encode(msg.signature),
    };

    serde_json::to_value(&t).map_err(DecodeError::Serde)
}

// #[cfg(test)]
// #[test]
// fn de() {
//     let hex = "12200000165c00000000000000000000000000000000000000000000000000000000";
//     let id = libp2p_core::PeerId::from_bytes(&hex::decode(hex).unwrap()).unwrap();
//     dbg!(id.to_base58());
// }
