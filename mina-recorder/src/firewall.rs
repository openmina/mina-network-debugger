use std::net::IpAddr;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct EnableWhitelist {
    pub ips: Vec<IpAddr>,
    pub ports: Vec<u16>,
}

pub enum FirewallCommand {
    EnableWhitelist(EnableWhitelist),
    DisableWhitelist,
}

pub mod stats {
    use std::{
        collections::BTreeMap,
        net::{SocketAddr, Ipv6Addr, IpAddr},
    };

    use ebpf_user::{kind::AppItem, HashMapRef, kind::AppItemKind};
    use serde::Serialize;

    #[derive(Clone, Copy, Debug, Serialize)]
    pub struct StatsBlocked {
        packets: u32,
        bytes: u32,
    }

    #[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
    pub struct StatsItem {
        src: SocketAddr,
        dst: SocketAddr,
    }

    pub struct StatsBlockedMap(pub HashMapRef<36, 8>);

    unsafe impl Send for StatsBlockedMap {}

    unsafe impl Sync for StatsBlockedMap {}

    impl StatsBlockedMap {
        pub fn list(&self) -> BTreeMap<StatsItem, StatsBlocked> {
            let mut list = BTreeMap::new();

            let fd = match self.0.kind() {
                AppItemKind::Map(map) => map.fd(),
                _ => unreachable!(),
            };
            let mut it = std::ptr::null();
            let mut next_key = [0; 36];
            while unsafe { libbpf_sys::bpf_map_get_next_key(fd, it, next_key.as_mut_ptr() as _) }
                == 0
            {
                let value = self.0.get(&next_key).unwrap();

                let src_ip = Ipv6Addr::from(<[u8; 16]>::try_from(&next_key[0..16]).unwrap());
                let src_ip = src_ip
                    .to_ipv4_mapped()
                    .map(IpAddr::from)
                    .unwrap_or(src_ip.into());
                let src_port = u16::from_be_bytes(next_key[16..18].try_into().unwrap());

                let dst_ip = Ipv6Addr::from(<[u8; 16]>::try_from(&next_key[18..34]).unwrap());
                let dst_ip = dst_ip
                    .to_ipv4_mapped()
                    .map(IpAddr::from)
                    .unwrap_or(src_ip.into());
                let dst_port = u16::from_be_bytes(next_key[34..36].try_into().unwrap());

                let key = StatsItem {
                    src: SocketAddr::new(src_ip, src_port),
                    dst: SocketAddr::new(dst_ip, dst_port),
                };
                let value = StatsBlocked {
                    packets: u32::from_ne_bytes(value[..4].try_into().unwrap()),
                    bytes: u32::from_ne_bytes(value[4..].try_into().unwrap()),
                };

                list.insert(key, value);
                it = &next_key as *const _ as _;
            }

            list
        }
    }
}
