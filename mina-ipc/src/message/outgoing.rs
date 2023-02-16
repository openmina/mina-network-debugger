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
    Irrelevant,
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
        peer_id: String,
        peer_host: String,
        peer_port: u16,
        data: Vec<u8>,
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
                        // TODO: use it to determine topic
                        x.get_subscription_id()?.get_id();

                        PushMessage::GossipReceived {
                            peer_id: sender.get_peer_id()?.get_id()?.to_owned(),
                            peer_host: sender.get_host()?.to_owned(),
                            peer_port: sender.get_libp2p_port(),
                            data: data.to_owned(),
                        }
                    }
                    _ => PushMessage::Irrelevant,
                };
                Ok(Self::PushMessage(msg))
            }
        }
    }
}
