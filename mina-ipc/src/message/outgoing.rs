use capnp::message::{Reader, ReaderSegments};

use crate::libp2p_ipc_capnp::{
    daemon_interface::{message, push_message},
    libp2p_helper_interface::{rpc_response, rpc_response_success},
};

// node <- libp2p
#[derive(Debug)]
pub enum Msg {
    Unknown(u16),
    RpcResponse(RpcResponse),
    PushMessage(PushMessage),
}

impl Msg {
    pub fn relevant(&self) -> bool {
        match self {
            Self::Unknown(_) => false,
            Self::RpcResponse(x) => x.relevant(),
            Self::PushMessage(x) => x.relevant(),
        }
    }
}

#[derive(Debug)]
pub enum RpcResponse {
    Unknown(u16),
    SuccessUnknown(u16),
    Error(String),
    Configure,
    GenerateKeypair {
        peer_id: String,
        public_key: Vec<u8>,
        secret_key: Vec<u8>,
    },
    OutgoingStream(OutgoingStream),
    SendStreamSuccess,
    SubscribeSuccess,
    AddStreamHandlerSuccess,
    Irrelevant,
}

#[derive(Debug)]
pub struct OutgoingStream {
    pub peer_id: String,
    pub peer_host: String,
    pub peer_port: u16,
    pub stream_id: u64,
}

impl RpcResponse {
    pub fn relevant(&self) -> bool {
        !matches!(
            self,
            Self::Irrelevant | Self::Unknown(_) | Self::SuccessUnknown(_)
        )
    }
}

#[derive(Debug)]
pub enum PushMessage {
    Unknown(u16),
    PeerConnected {
        peer_id: String,
    },
    PeerDisconnected {
        peer_id: String,
    },
    GossipReceived {
        subscription_id: u64,
        peer_id: String,
        peer_host: String,
        peer_port: u16,
        data: Vec<u8>,
    },
    IncomingStream {
        peer_id: String,
        peer_host: String,
        peer_port: u16,
        protocol: String,
        stream_id: u64,
    },
    StreamMessageReceived {
        data: Vec<u8>,
        stream_id: u64,
    },
    Irrelevant,
}

impl PushMessage {
    pub fn relevant(&self) -> bool {
        match self {
            Self::Unknown(_) | Self::Irrelevant => false,
            _ => true,
        }
    }
}

impl<S> TryFrom<Reader<S>> for Msg
where
    S: ReaderSegments,
{
    type Error = capnp::Error;

    fn try_from(value: Reader<S>) -> Result<Self, Self::Error> {
        let root = value.get_root::<message::Reader>()?;
        match root.which() {
            Err(x) => Ok(Self::Unknown(x.0)),
            Ok(message::RpcResponse(x)) => {
                let response = match x?.which() {
                    Err(x) => RpcResponse::Unknown(x.0),
                    Ok(rpc_response::Error(error)) => RpcResponse::Error(error?.to_owned()),
                    Ok(rpc_response::Success(x)) => match x?.which() {
                        Err(x) => RpcResponse::SuccessUnknown(x.0),
                        Ok(rpc_response_success::Configure(x)) => {
                            let _ = x?;
                            RpcResponse::Configure
                        }
                        Ok(rpc_response_success::GenerateKeypair(x)) => {
                            let x = x?.get_result()?;
                            let peer_id = x.get_peer_id()?.get_id()?.to_owned();
                            let public_key = x.get_public_key()?.to_owned();
                            let secret_key = x.get_private_key()?.to_owned();
                            RpcResponse::GenerateKeypair {
                                peer_id,
                                public_key,
                                secret_key,
                            }
                        }
                        Ok(rpc_response_success::OpenStream(x)) => {
                            let x = x?;
                            let sender = x.get_peer()?;

                            RpcResponse::OutgoingStream(OutgoingStream {
                                peer_id: sender.get_peer_id()?.get_id()?.to_owned(),
                                peer_host: sender.get_host()?.to_owned(),
                                peer_port: sender.get_libp2p_port(),
                                stream_id: x.get_stream_id()?.get_id(),
                            })
                        }
                        Ok(rpc_response_success::SendStream(x)) => {
                            x?;
                            RpcResponse::SendStreamSuccess
                        }
                        Ok(rpc_response_success::Subscribe(x)) => {
                            x?;
                            RpcResponse::SubscribeSuccess
                        }
                        Ok(rpc_response_success::AddStreamHandler(x)) => {
                            x?;
                            RpcResponse::AddStreamHandlerSuccess
                        }
                        _ => RpcResponse::Irrelevant,
                    },
                };
                Ok(Self::RpcResponse(response))
            }
            Ok(message::PushMessage(x)) => {
                let msg = match x?.which() {
                    Err(x) => PushMessage::Unknown(x.0),
                    Ok(push_message::PeerConnected(x)) => PushMessage::PeerConnected {
                        peer_id: x?.get_peer_id()?.get_id()?.to_owned(),
                    },
                    Ok(push_message::PeerDisconnected(x)) => PushMessage::PeerDisconnected {
                        peer_id: x?.get_peer_id()?.get_id()?.to_owned(),
                    },
                    Ok(push_message::GossipReceived(x)) => {
                        let x = x?;
                        let sender = x.get_sender()?;
                        let data = x.get_data()?;

                        PushMessage::GossipReceived {
                            subscription_id: x.get_subscription_id()?.get_id(),
                            peer_id: sender.get_peer_id()?.get_id()?.to_owned(),
                            peer_host: sender.get_host()?.to_owned(),
                            peer_port: sender.get_libp2p_port(),
                            data: data.to_owned(),
                        }
                    }
                    Ok(push_message::IncomingStream(x)) => {
                        let x = x?;
                        let sender = x.get_peer()?;

                        PushMessage::IncomingStream {
                            peer_id: sender.get_peer_id()?.get_id()?.to_owned(),
                            peer_host: sender.get_host()?.to_owned(),
                            peer_port: sender.get_libp2p_port(),
                            protocol: x.get_protocol()?.to_owned(),
                            stream_id: x.get_stream_id()?.get_id(),
                        }
                    }
                    Ok(push_message::StreamMessageReceived(x)) => {
                        let x = x?;
                        let msg = x.get_msg()?;
                        PushMessage::StreamMessageReceived {
                            data: msg.get_data()?.to_owned(),
                            stream_id: msg.get_stream_id()?.get_id(),
                        }
                    }
                    _ => PushMessage::Irrelevant,
                };
                Ok(Self::PushMessage(msg))
            }
        }
    }
}
