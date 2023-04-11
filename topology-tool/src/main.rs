use std::{
    time::Duration,
    net::{IpAddr, SocketAddr},
};

use reqwest::{
    Url,
    blocking::{ClientBuilder, Client},
};
use serde::Deserialize;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Args {
    #[structopt(long)]
    port: u16,
    #[structopt(long)]
    nodes: u16,
    #[structopt(long)]
    snarkers: u16,
    #[structopt(long)]
    prods: u16,
    #[structopt(long)]
    prod0s: u16,
    #[structopt(subcommand)]
    command: Command,
}

#[derive(StructOpt)]
enum Command {
    EnableFirewall,
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
}

fn enable_firewall(client: &Client, port: u16, graph: &[NodeInfo]) {
    fn debugger_firewall_enable_url(port: u16, name: &str) -> Url {
        format!("http://1.k8.openmina.com:{port}/{name}/bpf-debugger/firewall/whitelist/enable")
            .parse()
            .unwrap()
    }

    let ports = [10909];

    let len = graph.len();
    let left = graph
        .iter()
        .take(len / 2)
        .map(|x| ports.into_iter().map(|port| SocketAddr::new(x.ip, port)))
        .flatten()
        .collect::<Vec<_>>();
    let right = graph
        .iter()
        .skip(len / 2)
        .map(|x| ports.into_iter().map(|port| SocketAddr::new(x.ip, port)))
        .flatten()
        .collect::<Vec<_>>();

    for node in graph {
        let whitelist = if left.iter().find(|x| x.ip() == node.ip).is_some() {
            &left
        } else {
            &right
        };
        log::info!("{whitelist:?}");
        let whitelist = serde_json::to_string(whitelist).unwrap();
        let url = debugger_firewall_enable_url(port, &node.name);
        match client
            .post(url.clone())
            .header("content-type", "application/json")
            .body(whitelist)
            .send()
        {
            Ok(response) => log::info!("{url}: {}", response.status()),
            Err(err) => log::error!("{url}: {err}"),
        }
    }
}

fn disable_firewall(client: &Client, port: u16, graph: &[NodeInfo]) {
    fn debugger_firewall_disable_url(port: u16, name: &str) -> Url {
        format!("http://1.k8.openmina.com:{port}/{name}/bpf-debugger/firewall/whitelist/disable")
            .parse()
            .unwrap()
    }

    for node in graph {
        let url = debugger_firewall_disable_url(port, &node.name);
        match client.post(url.clone()).send() {
            Ok(response) => log::info!("{url}: {}", response.status()),
            Err(err) => log::error!("{url}: {err}"),
        }
    }
}

fn query_peer(client: &Client, port: u16, name: &str) -> anyhow::Result<NodeInfo> {
    fn graphql_url(port: u16, name: &str) -> Url {
        format!("http://1.k8.openmina.com:{port}/{name}/graphql")
            .parse()
            .unwrap()
    }

    let response = client
        .post(graphql_url(port, &name))
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
        .post(graphql_url(port, &name))
        .body(r#"{"query":"query MyQuery { getPeers { host } }"}"#)
        .header("content-type", "application/json")
        .send()?
        .text()?;
    let response = serde_json::from_str::<GraphqlResponse>(&response)?;
    let peers = match response.data {
        GraphqlResponseData::GetPeers(x) => x.into_iter().map(|PeerInfo { host }| host).collect(),
        _ => panic!("unexpected response"),
    };

    Ok(NodeInfo {
        ip,
        name: name.to_owned(),
        peers,
    })
}

fn show_graph(graph: &[NodeInfo]) -> usize {
    use std::collections::BTreeMap;
    use petgraph::{prelude::DiGraph, algo, dot};

    let mut pet = DiGraph::new();
    let mut ips = BTreeMap::new();
    let ip_to_name = graph
        .iter()
        .map(|NodeInfo { ip, name, .. }| (*ip, name.clone()))
        .collect::<BTreeMap<_, _>>();

    for NodeInfo { ip, name, peers } in graph {
        let ip_a = ip;

        let a = *ips
            .entry(*ip_a)
            .or_insert_with(|| pet.add_node(name.clone()));

        for &ip_b in peers {
            let Some(name) = ip_to_name.get(&ip_b) else {
                continue;
            };

            let b = *ips
                .entry(ip_b)
                .or_insert_with(|| pet.add_node(name.clone()));
            pet.add_edge(a, b, ());
        }
    }

    let config = [dot::Config::EdgeNoLabel];

    println!("{:?}", dot::Dot::with_config(&pet, &config));
    algo::connected_components(&pet)
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .format(|buf, record| {
            use std::{io::Write, time::SystemTime};
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
        port,
        nodes,
        snarkers,
        prods,
        prod0s,
        command,
    } = Args::from_args();
    let client = ClientBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let nodes_names = (0..nodes).map(|i| format!("node{}", i + 1));
    let snarkers_names = (0..snarkers).map(|i| format!("snarker{}", i + 1));
    let prods_names = (1..prods).map(|i| format!("prod{}", i + 1));
    let prod0s_names = (0..prod0s).map(|i| format!("prod0{}", i + 1));
    let names = nodes_names
        .chain(snarkers_names)
        .chain(prods_names)
        .chain(prod0s_names)
        .chain(std::iter::once("seed1".to_owned()));

    let graph = names
        .filter_map(|name| match query_peer(&client, port, &name) {
            Ok(v) => Some(v),
            Err(err) => {
                log::error!("name {name}, error: {err}");
                None
            }
        })
        .collect::<Vec<_>>();

    match command {
        Command::EnableFirewall => enable_firewall(&client, port, &graph),
        Command::DisableFirewall => disable_firewall(&client, port, &graph),
        Command::ShowGraph {
            expected_components,
        } => {
            let components = show_graph(&graph);
            if let Some(expected_components) = expected_components {
                if expected_components != components {
                    log::error!("fail, expected components: {expected_components}, actual components: {components}");
                }
            }
        }
    }

    Ok(())
}
