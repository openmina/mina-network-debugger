use std::{fs, thread, time::{Duration, SystemTime}, sync::mpsc, path::Path, net::{SocketAddr, IpAddr}};

use mina_ipc::message::{outgoing::PushMessage, Config};
use reqwest::{blocking::{ClientBuilder, Client}, Url};
use serde::{Serialize, Deserialize};

use crate::{libp2p_helper::Process, registry::{server, messages::{Registered, MockSplitReport, MockSplitEvent}}};

pub const PEER_PORT: u16 = 8302;

pub fn run(
    registry: &str,
    build_number: u32,
) -> anyhow::Result<()> {
    // spawn libp2p_helper, this will trigger bpf debugger
    let (mut process, rx) = Process::spawn();

    // try create an http client, stop the libp2p_helper and thus the bpf debugger if failed
    let client = ClientBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let client = match client {
        Ok(v) => v,
        Err(err) => {
            process.stop()?;
            log::error!("{err}");
            return Err(err.into());
        }
    };

    let registry = format!("http://{registry}:{}", server::PORT)
        .parse::<Url>()
        .expect("hostname must be valid");

    let result = run_inner(
        &registry,
        &client,
        build_number,
        &mut process,
        rx,
    );

    // ensure everything is stopped regardless `run_inner` succeeded or not
    let (_ipc, _status_code) = process.stop().expect("can check debuggers output");

    let report = result?;
    let mock_json = serde_json::to_string(&report)?;

    let url = registry.join(&format!("mock_split_report?build_number={build_number}"))?;
    // TODO: check
    let _status = client.post(url).body(mock_json).send()?.status();

    Ok(())
}

fn run_inner(registry: &Url, client: &Client, build_number: u32, process: &mut Process, rx: mpsc::Receiver<PushMessage>) -> anyhow::Result<MockSplitReport> {
    // register on the registry
    let url = registry.join(&format!("register?build_number={build_number}"))?;
    let response = serde_json::from_reader::<_, Registered>(client.get(url).send()?)?;

    let this_addr = response.external;

    let root = AsRef::<Path>::as_ref("/root/.mina-split-test");
    fs::create_dir_all(root)?;

    let listen_on = format!("/ip4/0.0.0.0/tcp/{PEER_PORT}");
    let private_key = hex::decode(response.secret_key)?;
    let peers_addr = response
        .peers
        .iter()
        .map(|(address, peer_id)| format!("/dns4/{address}/tcp/{PEER_PORT}/p2p/{peer_id}"))
        .collect::<Vec<String>>();
    let external_multiaddr = format!("/dns4/{this_addr}/tcp/{PEER_PORT}");
    let topic = "coda/test-split/0.0.1";

    let config = Config::new(
        &root.display().to_string(),
        &private_key,
        "00000000000000000000000066616b65206e6574776f726b00000000deadbeef",
        &[&listen_on],
        &external_multiaddr,
        &peers_addr,
        &[&[topic]],
    );
    process.configure(config)?;
    process.subscribe(0, topic)?;

    // split the network into two connected components
    #[derive(Default, Serialize, Deserialize)]
    struct Components {
        left: Vec<SocketAddr>,
        right: Vec<SocketAddr>,
    }

    impl Components {
        fn apply(&self, this_addr: IpAddr, client: &Client) -> anyhow::Result<()> {
            let url = "http://127.0.0.1:8000/firewall/whitelist";
            let whitelist = if self.left.contains(&SocketAddr::new(this_addr, PEER_PORT)) {
                &self.left
            } else {
                &self.right
            };
            let whitelist = serde_json::to_string(&whitelist)?;
            let _status = client.post(url).body(whitelist).send()?.status();

            Ok(())
        }
    }

    fn reunite(client: &Client) -> anyhow::Result<()> {
        let url = "http://127.0.0.1:8000/firewall/whitelist/clear";
        let _status = client.post(url).send()?.status();

        Ok(())
    }

    let (leader_command_tx, leader_command_rx) = mpsc::channel::<Components>();

    let receiver = thread::spawn(move || {
        let mut events = vec![];
        while let Ok(msg) = rx.recv() {
            match msg {
                PushMessage::PeerConnected { peer_id } => {
                    let timestamp = SystemTime::now();
                    events.push(MockSplitEvent::Connected { peer_id, timestamp })
                }
                PushMessage::PeerDisconnected { peer_id } => {
                    let timestamp = SystemTime::now();
                    events.push(MockSplitEvent::Disconnected { peer_id, timestamp })
                }
                PushMessage::GossipReceived { data, .. } => {
                    let components = serde_json::from_str(std::str::from_utf8(&data).unwrap()).unwrap();
                    leader_command_tx.send(components).unwrap();
                }
                _ => (),
            }
        }
        log::info!("join receiver");
        events
    });

    let peers_before = if response.leader {
        // if this node is a leader

        // wait some time until peers connect each other
        thread::sleep(Duration::from_secs(30));
        let peers = process.list_peers()?;

        // split the network into two half
        let left = response.peers.iter().take(response.peers.len() / 2).map(|(ip, _)| SocketAddr::new(*ip, PEER_PORT)).collect();
        let right = response.peers.iter().skip(response.peers.len() / 2).map(|(ip, _)| SocketAddr::new(*ip, PEER_PORT)).collect();
        let components = Components { left, right };
        let data = serde_json::to_string(&components)?.into_bytes();
        process.publish(topic.to_owned(), data)?;

        // apply the whitelist for itself
        components.apply(this_addr, client)?;

        peers.unwrap_or_default()
    } else {
        // if this node is not a leader, the job is much simpler
        // wait the command from the leader and apply it
        let components = leader_command_rx.recv()?;
        let peers = process.list_peers()?;
        components.apply(this_addr, client)?;
        peers.unwrap_or_default()
    };
    let split_time = SystemTime::now();

    // wait until peers disconnect each other
    thread::sleep(Duration::from_secs(30));
    let peers_after_split = process.list_peers()?.unwrap_or_default();

    // clear whitelist
    reunite(client)?;
    let reunite_time = SystemTime::now();

    // wait until peers connect each other again
    thread::sleep(Duration::from_secs(30));
    let peers_after_reunite = process.list_peers()?.unwrap_or_default();
    thread::sleep(Duration::from_secs(10));

    process.stop_receiving();
    let events = receiver.join().unwrap();

    Ok(MockSplitReport {
        peers_before,
        split_time,
        peers_after_split,
        reunite_time,
        peers_after_reunite,
        events,
    })
}
