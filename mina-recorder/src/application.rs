use std::{
    env,
    sync::{mpsc, Mutex, Arc},
    collections::BTreeMap,
    net::{IpAddr, Ipv6Addr, SocketAddr},
};

use ebpf_user::{
    kind::{AppItem, AppItemKind},
    HashMapRef,
};

use serde::{Serialize, Deserialize};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct StatsBlocked {
    pub packets: u32,
    pub bytes: u32,
}

#[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct StatsItem {
    pub src: SocketAddr,
    pub dst: SocketAddr,
}

#[derive(Deserialize)]
pub struct EnableWhitelist {
    pub ips: Vec<IpAddr>,
    pub ports: Vec<u16>,
}

enum ApplicationCommand {
    EnableWhitelist(EnableWhitelist),
    DisableWhitelist,
    GetFirewallStats,
    Terminate,
}

#[derive(Clone)]
pub struct Application {
    ctx: mpsc::SyncSender<ApplicationCommand>,
    drx: Arc<Mutex<mpsc::Receiver<BTreeMap<StatsItem, StatsBlocked>>>>,
}

/// It is !Send, so will block thread where created
pub struct ApplicationServer {
    whitelist: HashMapRef<16, 4>,
    whitelist_ports: HashMapRef<2, 4>,
    blocked: HashMapRef<36, 8>,
    crx: mpsc::Receiver<ApplicationCommand>,
    dtx: mpsc::Sender<BTreeMap<StatsItem, StatsBlocked>>,
}

impl Application {
    pub fn enable_firewall(&self, list: EnableWhitelist) {
        self.ctx
            .send(ApplicationCommand::EnableWhitelist(list))
            .unwrap_or_default();
    }

    pub fn disable_firewall(&self) {
        self.ctx
            .send(ApplicationCommand::DisableWhitelist)
            .unwrap_or_default();
    }

    pub fn get_firewall_stats(&self) -> BTreeMap<StatsItem, StatsBlocked> {
        let drx = self
            .drx
            .lock()
            .expect("must not panic while hold this lock");
        self.ctx
            .send(ApplicationCommand::GetFirewallStats)
            .unwrap_or_default();
        drx.recv().unwrap_or_default()
    }

    pub fn terminate(&self) {
        self.ctx
            .send(ApplicationCommand::Terminate)
            .unwrap_or_default();
    }
}

impl ApplicationServer {
    fn clear_whitelist(&self) {
        let fd = match self.whitelist.kind() {
            AppItemKind::Map(map) => map.fd(),
            _ => unreachable!(),
        };

        // remove all entries
        let mut key = std::ptr::null();
        let mut next_key = [0; 16];
        while unsafe { libbpf_sys::bpf_map_get_next_key(fd, key, next_key.as_mut_ptr() as _) } == 0
        {
            self.whitelist.remove(&next_key).unwrap();
            key = &next_key as *const _ as _;
        }

        let fd = match self.whitelist_ports.kind() {
            AppItemKind::Map(map) => map.fd(),
            _ => unreachable!(),
        };

        // remove all entries
        let mut key = std::ptr::null();
        let mut next_key = [0; 2];
        while unsafe { libbpf_sys::bpf_map_get_next_key(fd, key, next_key.as_mut_ptr() as _) } == 0
        {
            self.whitelist_ports.remove(&next_key).unwrap();
            key = &next_key as *const _ as _;
        }
    }

    fn list(&self) -> BTreeMap<StatsItem, StatsBlocked> {
        let mut list = BTreeMap::new();

        let fd = match self.blocked.kind() {
            AppItemKind::Map(map) => map.fd(),
            _ => unreachable!(),
        };
        let mut it = std::ptr::null();
        let mut next_key = [0; 36];
        while unsafe { libbpf_sys::bpf_map_get_next_key(fd, it, next_key.as_mut_ptr() as _) } == 0 {
            let value = self.blocked.get(&next_key).unwrap();

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

    pub fn run(mut self) {
        while let Ok(command) = self.crx.recv() {
            match command {
                ApplicationCommand::EnableWhitelist(EnableWhitelist { mut ips, ports }) => {
                    self.clear_whitelist();

                    // remove mark that whitelist is disabled
                    self.whitelist.remove(&[0; 16]).unwrap_or_default();

                    if let Ok(list) = env::var("FIREWALL_DEFAULT_WHITELIST") {
                        for ip in list.split(',').filter_map(|s| s.parse::<IpAddr>().ok()) {
                            ips.push(ip);
                        }
                    }
                    for &addr in &ips {
                        let ipv6 = match addr {
                            IpAddr::V4(ip) => ip.to_ipv6_mapped(),
                            IpAddr::V6(ip) => ip,
                        };
                        self.whitelist.insert(ipv6.octets(), [0, 0, 0, 1]).unwrap();
                    }
                    for &port in &ports {
                        self.whitelist_ports
                            .insert(port.to_be_bytes(), [0, 0, 0, 1])
                            .unwrap();
                    }
                    log::info!("firewall: whitelist {ips:?}, ports: {ports:?}");
                }
                ApplicationCommand::DisableWhitelist => {
                    self.clear_whitelist();

                    // insert mark that whitelist is disabled
                    self.whitelist.insert([0; 16], [0, 0, 0, 1]).unwrap();

                    log::info!("firewall: whitelist disable");
                }
                ApplicationCommand::GetFirewallStats => {
                    self.dtx.send(self.list()).unwrap_or_default();
                }
                ApplicationCommand::Terminate => break,
            }
        }
    }
}

pub fn new(
    whitelist: HashMapRef<16, 4>,
    whitelist_ports: HashMapRef<2, 4>,
    blocked: HashMapRef<36, 8>,
) -> (Application, ApplicationServer) {
    let (ctx, crx) = mpsc::sync_channel(256);
    let (dtx, drx) = mpsc::channel();
    let drx = Arc::new(Mutex::new(drx));

    (
        Application { ctx, drx },
        ApplicationServer {
            whitelist,
            whitelist_ports,
            blocked,
            crx,
            dtx,
        },
    )
}
