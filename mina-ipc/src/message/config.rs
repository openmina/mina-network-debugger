use capnp::message::Builder;

use crate::libp2p_ipc_capnp::{gating_config, libp2p_config, topic_level};
use crate::message::CapnpEncode;

#[derive(Debug)]
pub struct Config {
    pub statedir: String,
    pub private_key: Vec<u8>,
    pub network_id: String,
    pub listen_on: Vec<String>,
    pub metrics_port: u16,
    pub external_multiaddr: String,
    pub unsafe_no_trust_ip: bool,
    pub flood: bool,
    pub peer_exchange: bool,
    pub direct_peers: Vec<String>,
    pub seed_peers: Vec<String>,
    pub gating_config: GatingConfig,
    pub max_connections: u32,
    pub validation_queue_size: u32,
    pub peer_protection_ratio: f32,
    pub min_connections: u32,
    pub known_private_ip_nets: Vec<String>,
    pub topic_config: Vec<Vec<String>>,
}

impl Config {
    pub fn new<P>(
        statedir: &str,
        private_key: &[u8],
        network_id: &str,
        listen_on: &[&str],
        external_multiaddr: &str,
        seed_peers: &[P],
        topic_config: &[&[&str]],
    ) -> Self
    where
        P: AsRef<str>,
    {
        Config {
            statedir: statedir.to_owned(),
            private_key: private_key.to_owned(),
            network_id: network_id.to_owned(),
            listen_on: listen_on.iter().map(|&a| a.to_owned()).collect(),
            metrics_port: 0,
            external_multiaddr: external_multiaddr.to_owned(),
            unsafe_no_trust_ip: false,
            flood: false,
            peer_exchange: false,
            direct_peers: vec![],
            seed_peers: seed_peers.iter().map(|a| a.as_ref().to_owned()).collect(),
            gating_config: GatingConfig::default(),
            max_connections: 50,
            validation_queue_size: 150,
            peer_protection_ratio: 0.2,
            min_connections: 20,
            known_private_ip_nets: vec![],
            topic_config: topic_config
                .iter()
                .map(|&a| a.iter().map(|&a| a.to_owned()).collect())
                .collect(),
        }
    }
}

impl<'a> CapnpEncode<'a> for Config {
    type Builder = libp2p_config::Builder<'a>;

    fn build(&self, mut builder: Self::Builder) -> capnp::Result<()> {
        builder.set_statedir(&self.statedir);
        builder.set_private_key(&self.private_key);
        builder.set_network_id(&self.network_id);
        let mut t = builder.reborrow().init_listen_on(self.listen_on.len() as _);
        for (index, value) in self.listen_on.iter().enumerate() {
            t.reborrow().get(index as _).set_representation(value);
        }
        builder.set_metrics_port(self.metrics_port);
        builder
            .reborrow()
            .init_external_multiaddr()
            .set_representation(&self.external_multiaddr);
        builder.set_unsafe_no_trust_ip(self.unsafe_no_trust_ip);
        builder.set_flood(self.flood);
        builder.set_peer_exchange(self.peer_exchange);
        let mut t = builder
            .reborrow()
            .init_direct_peers(self.direct_peers.len() as _);
        for (index, value) in self.direct_peers.iter().enumerate() {
            t.reborrow().get(index as _).set_representation(value);
        }
        let mut t = builder
            .reborrow()
            .init_seed_peers(self.seed_peers.len() as _);
        for (index, value) in self.seed_peers.iter().enumerate() {
            t.reborrow().get(index as _).set_representation(value);
        }
        self.gating_config
            .build(builder.reborrow().init_gating_config())?;
        builder.set_max_connections(self.max_connections);
        builder.set_validation_queue_size(self.validation_queue_size);
        builder.set_peer_protection_ratio(self.peer_protection_ratio);
        builder.set_min_connections(self.min_connections);
        let mut t = builder
            .reborrow()
            .init_known_private_ip_nets(self.known_private_ip_nets.len() as _);
        for (index, value) in self.known_private_ip_nets.iter().enumerate() {
            t.reborrow().set(index as _, value);
        }
        let t = builder
            .reborrow()
            .init_topic_config(self.topic_config.len() as _);
        for (index, value) in self.topic_config.iter().enumerate() {
            let mut r = Builder::new_default();
            let mut root = r.init_root::<topic_level::Builder>();
            let mut topics = root.reborrow().init_topics(value.len() as _);
            for (index, value) in value.into_iter().enumerate() {
                topics.set(index as _, &value);
            }
            t.set_with_caveats(index as _, root.into_reader())?;
        }

        Ok(())
    }
}

impl<'a> TryFrom<libp2p_config::Reader<'a>> for Config {
    type Error = capnp::Error;

    fn try_from(value: libp2p_config::Reader) -> Result<Self, Self::Error> {
        Ok(Config {
            statedir: value.get_statedir()?.to_owned(),
            private_key: value.get_private_key()?.to_owned(),
            network_id: value.get_network_id()?.to_owned(),
            listen_on: value.get_listen_on()?.iter().try_fold(vec![], |mut v, x| {
                v.push(x.get_representation()?.to_owned());
                Ok::<_, capnp::Error>(v)
            })?,
            metrics_port: value.get_metrics_port(),
            external_multiaddr: value
                .get_external_multiaddr()?
                .get_representation()?
                .to_owned(),
            unsafe_no_trust_ip: value.get_unsafe_no_trust_ip(),
            flood: value.get_flood(),
            peer_exchange: value.get_peer_exchange(),
            direct_peers: value
                .get_direct_peers()?
                .iter()
                .try_fold(vec![], |mut v, x| {
                    v.push(x.get_representation()?.to_owned());
                    Ok::<_, capnp::Error>(v)
                })?,
            seed_peers: value
                .get_seed_peers()?
                .iter()
                .try_fold(vec![], |mut v, x| {
                    v.push(x.get_representation()?.to_owned());
                    Ok::<_, capnp::Error>(v)
                })?,
            gating_config: GatingConfig::try_from(value.get_gating_config()?)?,
            max_connections: value.get_max_connections(),
            validation_queue_size: value.get_validation_queue_size(),
            peer_protection_ratio: value.get_peer_protection_ratio(),
            min_connections: value.get_min_connections(),
            known_private_ip_nets: value.get_known_private_ip_nets()?.iter().try_fold(
                vec![],
                |mut v, x| {
                    v.push(x?.to_owned());
                    Ok::<_, capnp::Error>(v)
                },
            )?,
            topic_config: value
                .get_topic_config()?
                .iter()
                .try_fold(vec![], |mut v, x| {
                    let y = x.get_topics()?.iter().try_fold(vec![], |mut v, x| {
                        v.push(x?.to_owned());
                        Ok::<_, capnp::Error>(v)
                    })?;
                    v.push(y);
                    Ok::<_, capnp::Error>(v)
                })?,
        })
    }
}

#[derive(Default, Debug)]
pub struct GatingConfig {
    pub banned_ips: Vec<String>,
    pub banned_peer_ids: Vec<String>,
    pub trusted_ips: Vec<String>,
    pub trusted_peer_ids: Vec<String>,
    pub isolate: bool,
}

impl<'a> CapnpEncode<'a> for GatingConfig {
    type Builder = gating_config::Builder<'a>;

    fn build(&self, mut builder: Self::Builder) -> capnp::Result<()> {
        let mut t = builder
            .reborrow()
            .init_banned_ips(self.banned_ips.len() as _);
        for (index, value) in self.banned_ips.iter().enumerate() {
            t.set(index as _, value);
        }
        let mut t = builder
            .reborrow()
            .init_banned_peer_ids(self.banned_peer_ids.len() as _);
        for (index, value) in self.banned_peer_ids.iter().enumerate() {
            t.reborrow().get(index as _).set_id(value);
        }
        let mut t = builder
            .reborrow()
            .init_trusted_ips(self.trusted_ips.len() as _);
        for (index, value) in self.trusted_ips.iter().enumerate() {
            t.set(index as _, value);
        }
        let mut t = builder
            .reborrow()
            .init_trusted_peer_ids(self.trusted_peer_ids.len() as _);
        for (index, value) in self.trusted_peer_ids.iter().enumerate() {
            t.reborrow().get(index as _).set_id(value);
        }
        builder.set_isolate(self.isolate);

        Ok(())
    }
}

impl<'a> TryFrom<gating_config::Reader<'a>> for GatingConfig {
    type Error = capnp::Error;

    fn try_from(value: gating_config::Reader<'a>) -> Result<Self, Self::Error> {
        Ok(GatingConfig {
            banned_ips: value
                .get_banned_ips()?
                .iter()
                .try_fold(vec![], |mut v, x| {
                    v.push(x?.to_owned());
                    Ok::<_, capnp::Error>(v)
                })?,
            banned_peer_ids: value
                .get_banned_peer_ids()?
                .iter()
                .try_fold(vec![], |mut v, x| {
                    v.push(x.get_id()?.to_owned());
                    Ok::<_, capnp::Error>(v)
                })?,
            trusted_ips: value
                .get_trusted_ips()?
                .iter()
                .try_fold(vec![], |mut v, x| {
                    v.push(x?.to_owned());
                    Ok::<_, capnp::Error>(v)
                })?,
            trusted_peer_ids: value.get_trusted_peer_ids()?.iter().try_fold(
                vec![],
                |mut v, x| {
                    v.push(x.get_id()?.to_owned());
                    Ok::<_, capnp::Error>(v)
                },
            )?,
            isolate: value.get_isolate(),
        })
    }
}
