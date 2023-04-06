use capnp::message::{Reader, ReaderSegments};

use crate::libp2p_ipc_capnp::libp2p_helper_interface::{message, rpc_request};

use super::{
    config::{Config, GatingConfig},
    CapnpEncode,
};

// node -> libp2p
#[derive(Debug)]
pub enum Msg {
    Unknown(u16),
    RpcRequest(RpcRequest),
    PushMessage,
}

impl Msg {
    pub fn relevant(&self) -> bool {
        match self {
            Self::Unknown(_) => false,
            Self::RpcRequest(x) => x.relevant(),
            Self::PushMessage => false,
        }
    }
}

#[derive(Debug)]
pub enum RpcRequest {
    Unknown(u16),
    Configure(Config),
    SetGatingConfig(GatingConfig),
    AddPeer { is_seed: bool, multiaddr: String },
    ListPeers,
    GenerateKeypair,
    Subscribe { id: u64, topic: String },
    Publish { topic: String, data: Vec<u8> },
    AddStreamHandler { protocol: String },
    OpenStream { peer_id: String, protocol: String },
    SendStream { data: Vec<u8>, stream_id: u64 },
    Irrelevant,
}

impl RpcRequest {
    pub fn relevant(&self) -> bool {
        !matches!(self, Self::Unknown(_) | Self::Irrelevant)
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
            Ok(message::RpcRequest(x)) => match x?.which() {
                Err(x) => Ok(Self::RpcRequest(RpcRequest::Unknown(x.0))),
                Ok(rpc_request::Configure(x)) => {
                    let config = Config::try_from(x?.get_config()?)?;
                    Ok(Self::RpcRequest(RpcRequest::Configure(config)))
                }
                Ok(rpc_request::SetGatingConfig(x)) => {
                    let config = GatingConfig::try_from(x?.get_gating_config()?)?;
                    Ok(Self::RpcRequest(RpcRequest::SetGatingConfig(config)))
                }
                Ok(rpc_request::AddPeer(x)) => {
                    let x = x?;
                    Ok(Self::RpcRequest(RpcRequest::AddPeer {
                        is_seed: x.get_is_seed(),
                        multiaddr: x.get_multiaddr()?.get_representation()?.to_owned(),
                    }))
                }
                Ok(rpc_request::OpenStream(x)) => {
                    let x = x?;
                    let sender = x.get_peer()?;

                    Ok(Self::RpcRequest(RpcRequest::OpenStream {
                        peer_id: sender.get_id()?.to_owned(),
                        protocol: x.get_protocol_id()?.to_owned(),
                    }))
                }
                Ok(rpc_request::SendStream(x)) => {
                    let x = x?;
                    let msg = x.get_msg()?;

                    Ok(Self::RpcRequest(RpcRequest::SendStream {
                        data: msg.get_data()?.to_owned(),
                        stream_id: msg.get_stream_id()?.get_id(),
                    }))
                }
                Ok(rpc_request::AddStreamHandler(x)) => {
                    let x = x?;
                    let protocol = x.get_protocol()?.to_owned();
                    Ok(Self::RpcRequest(RpcRequest::AddStreamHandler { protocol }))
                }
                _ => Ok(Self::RpcRequest(RpcRequest::Irrelevant)),
            },
            Ok(message::PushMessage(x)) => {
                let _ = x;
                Ok(Self::PushMessage)
            }
        }
    }
}

impl<'a> CapnpEncode<'a> for Msg {
    type Builder = message::Builder<'a>;

    fn build(&self, builder: Self::Builder) -> capnp::Result<()> {
        match self {
            Self::Unknown(_) => Ok(()),
            Self::RpcRequest(RpcRequest::Configure(config)) => {
                let builder = builder.init_rpc_request().init_configure().init_config();
                config.build(builder)
            }
            Self::RpcRequest(x) => {
                let builder = builder.init_rpc_request();
                match x {
                    RpcRequest::Unknown(_) => Ok(()),
                    RpcRequest::Configure(config) => {
                        config.build(builder.init_configure().init_config())
                    }
                    RpcRequest::SetGatingConfig(gating_config) => {
                        gating_config.build(builder.init_set_gating_config().init_gating_config())
                    }
                    RpcRequest::AddPeer { is_seed, multiaddr } => {
                        let mut builder = builder.init_add_peer();
                        builder.set_is_seed(*is_seed);
                        builder.init_multiaddr().set_representation(multiaddr);
                        Ok(())
                    }
                    RpcRequest::ListPeers => {
                        builder.init_list_peers();
                        Ok(())
                    }
                    RpcRequest::GenerateKeypair => {
                        builder.init_generate_keypair();
                        Ok(())
                    }
                    RpcRequest::Subscribe { id, topic } => {
                        let mut builder = builder.init_subscribe();
                        builder.reborrow().init_subscription_id().set_id(*id);
                        builder.set_topic(topic);
                        Ok(())
                    }
                    RpcRequest::Publish { topic, data } => {
                        let mut builder = builder.init_publish();
                        builder.set_topic(topic);
                        builder.set_data(data);
                        Ok(())
                    }
                    RpcRequest::OpenStream { peer_id, protocol } => {
                        let mut builder = builder.init_open_stream();
                        let mut peer = builder.reborrow().init_peer();
                        peer.set_id(peer_id);
                        builder.set_protocol_id(protocol);
                        Ok(())
                    }
                    RpcRequest::SendStream { data, stream_id } => {
                        let mut builder = builder.init_send_stream().init_msg();
                        builder.set_data(data);
                        builder.init_stream_id().set_id(*stream_id);
                        Ok(())
                    }
                    RpcRequest::AddStreamHandler { protocol } => {
                        let mut builder = builder.init_add_stream_handler();
                        builder.set_protocol(protocol);
                        Ok(())
                    }
                    RpcRequest::Irrelevant => Ok(()),
                }
            }
            Self::PushMessage => Ok(()),
        }
    }
}
