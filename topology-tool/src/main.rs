use std::{
    time::Duration,
    net::IpAddr,
    collections::{BTreeSet, BTreeMap},
};

use reqwest::{
    Url,
    blocking::{ClientBuilder, Client},
};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Args {
    // http://1.k8.openmina.com:31366
    #[structopt(long)]
    url: String,
    #[structopt(long)]
    nodes: Option<u16>,
    #[structopt(long)]
    snarkers: Option<u16>,
    #[structopt(long)]
    prods: Option<u16>,
    #[structopt(long)]
    prod0s: Option<u16>,
    #[structopt(long)]
    seeds: Option<u16>,
    #[structopt(subcommand)]
    command: Command,
}

#[derive(StructOpt)]
enum Command {
    EnableFirewall {
        // list of lists of name:ip
        #[structopt(long)]
        segments: Vec<String>,
    },
    DisableFirewall,
    ShowGraph {
        #[structopt(long)]
        expected_components: Option<usize>,
    },
}

#[derive(Deserialize, Debug)]
struct GraphqlResponse {
    data: GraphqlResponseData,
}

#[derive(Deserialize, Debug)]
#[non_exhaustive]
enum GraphqlResponseData {
    #[serde(rename = "getPeers")]
    GetPeers(Vec<PeerInfo>),
    #[serde(rename = "daemonStatus")]
    DaemonStatus(DaemonStatus),
}

#[derive(Deserialize, Debug)]
struct DaemonStatus {
    #[serde(rename = "addrsAndPorts")]
    addrs_and_ports: AddrsAndPorts,
}

#[derive(Deserialize, Debug)]
struct AddrsAndPorts {
    #[serde(rename = "externalIp")]
    external_ip: IpAddr,
}

#[derive(Deserialize, Debug)]
struct PeerInfo {
    host: IpAddr,
}

struct NodeInfo {
    ip: IpAddr,
    name: String,
    peers: Vec<IpAddr>,
    head: Option<String>,
}

impl NodeInfo {
    pub fn left(&self) -> bool {
        self.name.bytes().last().unwrap_or_default() % 2 == 0
    }
}

#[derive(Serialize, Debug)]
pub struct EnableWhitelist {
    pub ips: Vec<IpAddr>,
    pub ports: Vec<u16>,
}

fn debugger_firewall_enable_url(url: &str, name: &str) -> Url {
    format!("{url}/{name}/bpf-debugger/firewall/whitelist/enable")
        .parse()
        .unwrap()
}

fn enable_firewall_simple(client: &Client, url: String, segments: Vec<String>) {
    let mut names = vec![];
    let segments = segments
        .into_iter()
        .map(|s| {
            s.split(',')
                .filter_map(|s| {
                    let mut it = s.split(':');
                    let name = it.next()?.to_owned();
                    let ip = it.next()?.parse::<IpAddr>().ok()?;
                    names.push(name.clone());
                    Some((name, ip))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .collect::<Vec<_>>();

    let ports = [10909, 10001];

    for name in names {
        let (whitelist_names, ips) = segments
            .iter()
            .find(|x| x.contains_key(&name))
            .map(|segment| {
                (
                    segment.keys().cloned().collect::<Vec<_>>(),
                    segment.values().copied().collect(),
                )
            })
            .unwrap_or_default();
        let whitelist = EnableWhitelist {
            ips,
            ports: ports.to_vec(),
        };
        let whitelist_str = serde_json::to_string(&whitelist).unwrap();
        let url = debugger_firewall_enable_url(&url, &name);
        match client
            .post(url.clone())
            .header("content-type", "application/json")
            .body(whitelist_str)
            .send()
        {
            Ok(response) => {
                let status = response.status();
                log::info!("whitelist {whitelist_names:?} for {name}: {status}");
            }
            Err(err) => log::error!("whitelist {whitelist_names:?} for {name}: {err}"),
        }
    }
}

fn enable_firewall(client: &Client, url: String, graph: &[NodeInfo]) {
    let ports = [10909, 10001];

    let (left, left_names) = graph
        .iter()
        .filter_map(|x| if x.left() { Some(x) } else { None })
        .map(|x| (x.ip, x.name.clone()))
        .unzip::<_, _, Vec<_>, Vec<_>>();
    let (right, right_names) = graph
        .iter()
        .filter_map(|x| if !x.left() { Some(x) } else { None })
        .map(|x| (x.ip, x.name.clone()))
        .unzip::<_, _, Vec<_>, Vec<_>>();

    for node in graph {
        let whitelist = EnableWhitelist {
            ips: if node.left() {
                left.clone()
            } else {
                right.clone()
            },
            ports: ports.to_vec(),
        };
        let whitelist_names = if node.left() {
            &left_names
        } else {
            &right_names
        };
        let whitelist = serde_json::to_string(&whitelist).unwrap();
        let url = debugger_firewall_enable_url(&url, &node.name);
        match client
            .post(url.clone())
            .header("content-type", "application/json")
            .body(whitelist)
            .send()
        {
            Ok(response) => {
                let status = response.status();
                log::info!("whitelist {whitelist_names:?} for {}: {status}", node.name);
            }
            Err(err) => log::error!("whitelist {whitelist_names:?} for {}: {err}", node.name),
        }
    }
}

fn disable_firewall(client: &Client, url: String, names: impl Iterator<Item = String>) {
    fn debugger_firewall_disable_url(url: &str, name: &str) -> Url {
        format!("{url}/{name}/bpf-debugger/firewall/whitelist/disable")
            .parse()
            .unwrap()
    }

    for name in names {
        let url = debugger_firewall_disable_url(&url, &name);
        match client.post(url.clone()).send() {
            Ok(response) => log::info!("cleanup whitelist {name}: {}", response.status()),
            Err(err) => log::error!("cleanup whitelist {name}: {err}"),
        }
    }
}

fn query_peer(client: &Client, url: &str, name: &str) -> anyhow::Result<NodeInfo> {
    fn graphql_url(url: &str, name: &str) -> Url {
        format!("{url}/{name}/graphql").parse().unwrap()
    }

    fn debugger_url(url: &str, name: &str) -> Url {
        format!("{url}/{name}/bpf-debugger/libp2p_ipc/block/latest")
            .parse()
            .unwrap()
    }

    let response = client
        .post(graphql_url(url, &name))
        .body(r#"{"query":"query MyQuery { daemonStatus { addrsAndPorts { externalIp } } }"}"#)
        .header("content-type", "application/json")
        .send()?
        .text()?;
    let response = serde_json::from_str::<GraphqlResponse>(&response)?;
    let ip = match response.data {
        GraphqlResponseData::DaemonStatus(x) => x.addrs_and_ports.external_ip,
        _ => panic!("unexpected response"),
    };

    let response = client
        .post(graphql_url(url, &name))
        .body(r#"{"query":"query MyQuery { getPeers { host } }"}"#)
        .header("content-type", "application/json")
        .send()?
        .text()?;
    let response = serde_json::from_str::<GraphqlResponse>(&response)?;
    let peers = match response.data {
        GraphqlResponseData::GetPeers(x) => x.into_iter().map(|PeerInfo { host }| host).collect(),
        _ => panic!("unexpected response"),
    };

    #[derive(Deserialize)]
    pub struct CapnpTableRow {
        pub time_microseconds: u64,
        pub real_time_microseconds: u64,
        pub node_address: String,
        pub events: Vec<CapnpEventDecoded>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[serde(tag = "type")]
    pub enum CapnpEventDecoded {
        ReceivedGossip { hash: String },
        PublishGossip { hash: String },
    }

    let response = client
        .get(debugger_url(url, &name))
        .header("content-type", "application/json")
        .send()?
        .text()?;
    let mut head = None;
    if let Ok(response) = serde_json::from_str::<Vec<CapnpTableRow>>(&response) {
        for row in response {
            for event in row.events {
                let hash = match event {
                    CapnpEventDecoded::PublishGossip { hash } => hash,
                    CapnpEventDecoded::ReceivedGossip { hash } => hash,
                };
                head = Some(hash);
            }
        }
    }

    Ok(NodeInfo {
        ip,
        name: name.to_owned(),
        peers,
        head,
    })
}

fn show_graph(graph: &[NodeInfo]) -> usize {
    use petgraph::{prelude::DiGraph, algo, dot};

    let mut gr = DiGraph::new();
    let mut ips = BTreeMap::new();
    let ip_to_name = graph
        .iter()
        .map(|NodeInfo { ip, name, .. }| (*ip, name.clone()))
        .collect::<BTreeMap<_, _>>();

    for NodeInfo {
        ip, name, peers, ..
    } in graph
    {
        let ip_a = ip;

        let a = *ips
            .entry(*ip_a)
            .or_insert_with(|| gr.add_node(name.clone()));

        for &ip_b in peers {
            let Some(name) = ip_to_name.get(&ip_b) else {
                continue;
            };

            let b = *ips.entry(ip_b).or_insert_with(|| gr.add_node(name.clone()));
            gr.add_edge(a, b, ());
        }
    }

    let config = [dot::Config::EdgeNoLabel];

    println!("{:?}", dot::Dot::with_config(&gr, &config));
    algo::connected_components(&gr)
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .format(|buf, record| {
            use std::{time::SystemTime, io::Write};
            use time::OffsetDateTime;

            let (hour, minute, second, micro) = OffsetDateTime::from(SystemTime::now())
                .time()
                .as_hms_micro();
            writeln!(
                buf,
                "{hour:02}:{minute:02}:{second:02}.{micro:06} [{}] {}",
                record.level(),
                record.args()
            )
        })
        .filter(None, log::LevelFilter::Info)
        .init();

    let Args {
        url,
        nodes,
        snarkers,
        prods,
        prod0s,
        command,
        seeds,
    } = Args::from_args();
    let client = ClientBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let nodes_names = (0..nodes.unwrap_or_default()).map(|i| format!("node{}", i + 1));
    let snarkers_names = (0..snarkers.unwrap_or_default()).map(|i| format!("snarker{:03}", i + 1));
    let prods_names = (1..prods.unwrap_or_default()).map(|i| format!("prod{}", i + 1));
    let prod0s_names = (0..prod0s.unwrap_or_default()).map(|i| format!("prod0{}", i + 1));
    let seeds_names = (0..seeds.unwrap_or_default()).map(|i| format!("seed{}", i + 1));
    let names = nodes_names
        .chain(snarkers_names)
        .chain(prods_names)
        .chain(prod0s_names)
        .chain(seeds_names);

    match command {
        Command::EnableFirewall { segments } => {
            log::info!("Applying whitelists...");
            if segments.is_empty() {
                let graph = names
                    .filter_map(|name| match query_peer(&client, &url, &name) {
                        Ok(v) => Some(v),
                        Err(err) => {
                            log::error!("name {name}, error: {err}");
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                enable_firewall(&client, url, &graph);
            } else {
                enable_firewall_simple(&client, url, segments);
            }
            log::info!("Waiting for the split to occur at the network level of abstraction...");
        }
        Command::DisableFirewall => {
            log::info!("Removing whitelists...");
            disable_firewall(&client, url, names);
            log::info!("Whitelists removed. Waiting for network merge at the network level of abstraction...");
        }
        Command::ShowGraph {
            expected_components,
        } => {
            let graph = names
                .filter_map(|name| match query_peer(&client, &url, &name) {
                    Ok(v) => Some(v),
                    Err(err) => {
                        log::error!("name {name}, error: {err}");
                        None
                    }
                })
                .collect::<Vec<_>>();

            let components = show_graph(&graph);
            let _heads: BTreeSet<String> = graph
                .iter()
                .filter_map(|NodeInfo { name, head, .. }| {
                    let head = head.clone()?;
                    log::debug!("{name}: {head}");
                    Some(head)
                })
                .collect::<BTreeSet<String>>();

            // if expected_components is present, do the test
            if let Some(&expected_components) = expected_components.as_ref() {
                let mut failed = false;
                if expected_components == 1 {
                    log::info!(
                        "Expects the network to be connected, and graph has only one connected component."
                    );
                    if components == 1 {
                        log::info!("All nodes are connected. Network has only one component.");
                    } else {
                        log::error!("Test failed, components: {components}. Network has more than one component.");
                        failed = true;
                    }
                } else {
                    log::info!("Expect the split to occur at the network level of abstraction.");
                    if components == expected_components {
                        log::info!("Network has split. All nodes are still operational in their respective components.");
                    } else if components > expected_components {
                        log::warn!("Network has split. However netowrk has more components than expected, components: {components}.");
                    } else {
                        log::error!("Test failed, components: {components}.");
                        failed = true;
                    }
                }
                // if expected_components == 1 && heads.len() != 1 || expected_components > heads.len()
                // {
                //     log::error!(
                //         "fail, expected components: {expected_components}, actual components: {}",
                //         heads.len()
                //     );
                //     failed = true;
                // }
                if failed {
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
