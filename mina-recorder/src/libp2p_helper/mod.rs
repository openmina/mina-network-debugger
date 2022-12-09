use std::io;

pub fn process(pid: u32, incoming: bool, data: Vec<u8>) -> capnp::Result<()> {
    if incoming {
        process_request(pid, incoming, data)
    } else {
        process_response(pid, incoming, data)
    }
}

// TODO: figure out how to capture this, doesn't work for now
pub fn process_request(pid: u32, incoming: bool, data: Vec<u8>) -> capnp::Result<()> {
    use capnp::serialize;
    use crate::libp2p_ipc_capnp::libp2p_helper_interface::rpc_request;

    let incoming = if incoming { "<-" } else { "->" };

    let reader = io::BufReader::new(io::Cursor::new(data));
    let reader = serialize::read_message(reader, Default::default())?;

    let t = reader.get_root::<rpc_request::Reader>()?;

    match t.which()? {
        rpc_request::AddPeer(Ok(peer)) => {
            let addr = peer.get_multiaddr().unwrap().get_representation().unwrap();
            log::info!("capnp message {pid} {incoming} add_peer {addr}");
        }
        rpc_request::Publish(Ok(msg)) => {
            let topic = msg.get_topic().unwrap();
            log::info!("capnp message {pid} {incoming} publish {topic}");
        }
        _ => (),
    }

    Ok(())
}

pub fn process_response(pid: u32, incoming: bool, data: Vec<u8>) -> capnp::Result<()> {
    use capnp::serialize;
    use crate::libp2p_ipc_capnp::{
        daemon_interface::{message, push_message},
        libp2p_helper_interface::{rpc_response, rpc_response_success},
    };

    let incoming = if incoming { "<-" } else { "->" };

    let reader = io::BufReader::new(io::Cursor::new(data));
    let reader = serialize::read_message(reader, Default::default())?;

    let t = reader.get_root::<message::Reader>()?;

    match t.which()? {
        message::PushMessage(Ok(msg)) => match msg.which() {
            Ok(push_message::PeerConnected(Ok(peer))) => {
                let id = peer.get_peer_id().unwrap().get_id().unwrap();
                log::info!("capnp message {pid} {incoming} connected {id}");
            }
            Ok(push_message::PeerDisconnected(Ok(peer))) => {
                let id = peer.get_peer_id().unwrap().get_id().unwrap();
                log::info!("capnp message {pid} {incoming} disconnected {id}");
            }
            _ => (),
        },
        message::RpcResponse(Ok(response)) => match response.which() {
            Ok(rpc_response::Success(Ok(response))) => match response.which() {
                Ok(rpc_response_success::Listen(Ok(addresses))) => {
                    for addr in addresses.get_result().unwrap() {
                        let addr = addr.get_representation().unwrap();
                        log::info!("capnp message {pid} {incoming} listen {addr}");
                    }
                }
                _ => (),
            },
            _ => (),
        },
        _ => (),
    }

    Ok(())
}
