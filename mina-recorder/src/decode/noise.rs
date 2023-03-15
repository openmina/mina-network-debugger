use prost::{bytes::Bytes, Message};
use radiation::{Absorb, AbsorbExt, ParseError};
use serde::Serialize;

use libp2p_core::{
    PublicKey, PeerId,
    identity::{ed25519, secp256k1, ecdsa},
};

use super::{DecodeError, MessageType};

#[allow(clippy::derive_partial_eq_without_eq)]
mod pb {
    include!(concat!(env!("OUT_DIR"), "/envelope_proto.rs"));
}
#[allow(clippy::derive_partial_eq_without_eq)]
mod keys_proto {
    include!(concat!(env!("OUT_DIR"), "/keys_proto.rs"));
}

pub fn parse_types(bytes: &[u8]) -> Result<Vec<MessageType>, DecodeError> {
    let ty = if bytes.starts_with(b"mac_mismatch\x00\x00\x00\x00") {
        MessageType::FailedToDecrypt
    } else {
        MessageType::HandshakePayload
    };
    Ok(vec![ty])
}

pub fn parse(bytes: Vec<u8>, _: bool) -> Result<serde_json::Value, DecodeError> {
    #[derive(Serialize)]
    struct T {
        r#type: String,
        public_key: String,
        peer_id: String,
        signature: String,
        payload_type: String,
        payload: String,
    }

    #[derive(Serialize, Absorb)]
    struct F {
        this_decrypted: u64,
        this_failed: u64,
        total_decrypted: u64,
        total_failed: u64,
    }

    if bytes.starts_with(b"mac_mismatch\x00\x00\x00\x00") {
        let f = F::absorb_ext(&bytes[16..])
            .map_err(|err| err.map(ParseError::into_vec))
            .map_err(DecodeError::Parse)?;
        return serde_json::to_value(&f).map_err(DecodeError::Serde);
    }

    let buf = Bytes::from(bytes);
    let msg = pb::Envelope::decode(buf).map_err(DecodeError::Protobuf)?;

    let (r#type, public_key, peer_id) = match msg.public_key {
        None => ("".to_string(), "".to_string(), "".to_string()),
        Some(pk) => {
            let libp2p_pk = match pk.r#type() {
                keys_proto::KeyType::Rsa => return Err(DecodeError::Rsa),
                keys_proto::KeyType::Ed25519 => {
                    PublicKey::Ed25519(ed25519::PublicKey::decode(&pk.data)?)
                }
                keys_proto::KeyType::Secp256k1 => {
                    PublicKey::Secp256k1(secp256k1::PublicKey::decode(&pk.data)?)
                }
                keys_proto::KeyType::Ecdsa => {
                    PublicKey::Ecdsa(ecdsa::PublicKey::from_bytes(&pk.data)?)
                }
            };
            let id = PeerId::from_public_key(&libp2p_pk);
            (
                pk.r#type().as_str_name().to_string(),
                hex::encode(pk.data),
                id.to_base58(),
            )
        }
    };

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

#[cfg(test)]
#[test]
fn parse_peer_id_test() {
    let hex = "002408011220da91decf6f4c769327ca8ff03986e66fcfe6c59dca63d68c5ee359e52f8dc6e6";
    let data = hex::decode(hex).unwrap();
    let id = PeerId::from_bytes(&data).unwrap();
    assert_eq!(
        id.to_base58(),
        "12D3KooWQXa4AdCEZWe9QwoHnrANyMAXirozBdroNHkkvTMhT8bf"
    );
}
