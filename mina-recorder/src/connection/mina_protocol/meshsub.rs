use mina_serialization_types::v1::ExternalTransitionV1;
use bin_prot::encodable::BinProtEncodable;

use super::{DirectedId, HandleData, Cx};

mod gossipsub {
    include!(concat!(env!("OUT_DIR"), "/gossipsub.pb.rs"));
}

#[derive(Default)]
pub struct State {}

impl HandleData for State {
    type Output = String;

    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx) -> Self::Output {
        use prost::{bytes::Bytes, Message};

        let _ = (id, cx);

        let buf = Bytes::from(bytes.to_vec());
        let msg = gossipsub::Rpc::decode_length_delimited(buf).unwrap();

        msg.publish
            .into_iter()
            .filter_map(|msg| msg.data)
            .map(|data| {
                let x = if data[8] == 0 {
                    Some(ExternalTransitionV1::try_decode_binprot(&data[9..]).unwrap())
                } else {
                    None
                };
                format!("{x:?}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
#[test]
fn meshsub_msg() {
    use prost::{bytes::Bytes, Message as _};

    let buf = Bytes::from(hex::decode(include_str!("meshsub_test.hex")).unwrap());
    let msg = gossipsub::Rpc::decode_length_delimited(buf).unwrap();
    for a in msg.publish
        .into_iter()
        .filter_map(|msg| msg.data)
        .map(|data| {
            if data[8] == 0 {
                Some(ExternalTransitionV1::try_decode_binprot(&data[9..]).unwrap())
            } else {
                None
            }
        })
    {
        println!("{a:?}");
    }
}
