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
