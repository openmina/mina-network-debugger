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
    GenerateKeypair,
    Subscribe { id: u64, topic: String },
    Publish { topic: String, data: Vec<u8> },
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
                    RpcRequest::Irrelevant => Ok(()),
                }
            }
            Self::PushMessage => Ok(()),
        }
    }
}
