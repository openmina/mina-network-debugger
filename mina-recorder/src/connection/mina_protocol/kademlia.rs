use std::fmt;

use super::{DirectedId, HandleData, Cx};

#[allow(clippy::derive_partial_eq_without_eq)]
mod pb {
    include!(concat!(env!("OUT_DIR"), "/kad.pb.rs"));
}

// pub enum Output {
//     Nothing,
//     FindNodeRequest {
//         // public key?
//         peer_id: Vec<u8>,
//     },
//     FindNodeResponse {
//         peers: Vec<pb::message::Peer>,
//     }
// }

// impl fmt::Display for Output {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         match self {
//             Output::Nothing => write!(f, "not impl"),
//             Output::FindNodeRequest { peer_id } => {
//                 write!(f, "FindNode request {}", hex::encode(peer_id))
//             }
//             Output::FindNodeResponse { peers } => {
//                 write!(f, "FindNode response")?;
//                 for peer in peers {
//                     write!(f, " Peer(id: {}, cn: {:?})", hex::encode(&peer.id), peer.connection())?;
//                 }
//                 Ok(())
//             }
//         }
//     }
// }

pub struct RawOutput(pb::Message);

impl fmt::Display for RawOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Default)]
pub struct State {}

impl HandleData for State {
    type Output = RawOutput;

    #[inline(never)]
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        use prost::{bytes::Bytes, Message};

        let _ = (id, cx);

        let buf = Bytes::from(bytes.to_vec());
        let msg = <pb::Message as Message>::decode_length_delimited(buf).unwrap();

        // match msg.r#type() {
        //     pb::message::MessageType::FindNode => {
        //         if msg.closer_peers.is_empty() {
        //             Output::FindNodeRequest { peer_id: msg.key }
        //         } else {
        //             Output::FindNodeResponse {
        //                 peers: msg.closer_peers,
        //             }
        //         }
        //     }
        //     _ => Output::Nothing,
        // }
        RawOutput(msg)
    }
}
