use std::io::Cursor;

use serde::Serialize;
use mina_p2p_messages::rpc::MessageHeader;
use binprot::BinProtRead;

use super::{DecodeError, MessageType};

const LIST: [&'static str; 11] = [
    "__Versioned_rpc.Menu",
    "get_some_initial_peers",
    "get_staged_ledger_aux_and_pending_coinbases_at_hash",
    "answer_sync_ledger_query",
    "get_best_tip",
    "get_ancestry",
    "Get_transition_knowledge",
    "get_transition_chain",
    "get_transition_chain_proof",
    "ban_notify",
    "get_epoch_ledger",
];

pub fn parse_types(bytes: &[u8]) -> Result<Vec<MessageType>, DecodeError> {
    fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|window| window == needle)
    }

    let mut tys = vec![];
    let mut bytes = bytes;
    'outer: loop {
        for candidate in LIST {
            if let Some(pos) = find_subsequence(bytes, candidate.as_bytes()) {
                bytes = &bytes[(pos + candidate.len())..];
                tys.push(MessageType::Rpc { tag: candidate.to_string() });
                continue 'outer;
            }
        }
        break;
    }

    Ok(tys)
}

pub fn parse(bytes: Vec<u8>, preview: bool) -> Result<serde_json::Value, DecodeError> {
    #[derive(Serialize)]
    #[serde(rename_all = "snake_case")]
    #[serde(tag = "type")]
    enum Msg {
        Heartbeat,
        Request {
            tag: String,
            version: i32,
            id: i64,
        },
        Response {
            id: i64,
        },
    }

    if bytes.len() < 8 {
        return Ok(serde_json::Value::Null);
    }

    let len = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
    if bytes.len() < 8 + len {
        return Ok(serde_json::Value::Null);
    }
    let mut stream = Cursor::new(&bytes[8..(8 + len)]);
    let msg = MessageHeader::binprot_read(&mut stream).map_err(DecodeError::BinProt)?;
    let (preview_msg, t) = match msg {
        MessageHeader::Heartbeat => (serde_json::Value::Null, Msg::Heartbeat),
        MessageHeader::Query(q) => {
            let tag = String::from_utf8(q.tag.as_ref().to_vec()).unwrap();
            (
                serde_json::Value::String(tag.clone()),
                Msg::Request {
                    tag,
                    version: q.version,
                    id: q.id,
                },
            )
        },
        MessageHeader::Response(r) => (
            serde_json::to_value(&r.id).unwrap(),
            Msg::Response { id: r.id },
        ),
    };

    if preview {
        Ok(preview_msg)
    } else {
        serde_json::to_value(&t).map_err(DecodeError::Serde)
    }
}
