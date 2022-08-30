use serde::Serialize;

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
    struct Preview {
        tag: String,
    }

    if preview {
        let t = parse_types(&bytes)?
            .into_iter()
            .filter_map(|m| if let MessageType::Rpc { tag } = m {
                Some(Preview { tag })
            } else {
                None
            })
            .collect::<Vec<_>>();

        serde_json::to_value(&t).map_err(DecodeError::Serde)
    } else {
        Ok(serde_json::Value::Null)
    }
}
