use std::{fs, thread, time::SystemTime, sync::mpsc, path::Path};

use mina_ipc::message::{outgoing::PushMessage, Config};
use reqwest::{blocking::{ClientBuilder, Client}, Url};

use crate::{libp2p_helper::Process, registry::{server, messages::{Registered, MockSplitReport}}};

pub const PEER_PORT: u16 = 8302;

pub mod time {
    use std::time::Duration;

    // Timeout for the registry to perform RPC on the debugger.
    pub const REGISTRY_TIMEOUT: Duration = Duration::from_secs(10);

    // Registry is waiting for nodes to register for split.
    // Registry know how many nodes should participate and will waiting all nodes,
    // unless the timeout occur. The timeout may occur because some nodes failed to start
    // (because of DNS failure) and thus, some nodes will not participate in the simulation.
    pub const WAIT_TIMEOUT: Duration = Duration::from_secs(40);

    // Timeout for mock node to register for split on the registry server.
    // Must be much bigger then `REGISTRY_TIMEOUT`.
    pub const MOCK_TIMEOUT: Duration = Duration::from_secs(120);

    // Time for newly created node to connect to other nodes.
    pub const INITIALIZATION: Duration = Duration::from_secs(20);

    // Time shift between mock node and registry.
    // The actual split happens when all nodes registered for the split plus this shift.
    pub const SHIFT: Duration = Duration::from_secs(10);

    pub const SPLIT: Duration = Duration::from_secs(90);

    pub const REUNITE: Duration = Duration::from_secs(90);
}

pub fn run(
    registry: &str,
    build_number: u32,
) -> anyhow::Result<()> {
    // spawn libp2p_helper, this will trigger bpf debugger
    let (mut process, rx) = Process::spawn();

    // try create an http client, stop the libp2p_helper and thus the bpf debugger if failed
    let client = ClientBuilder::new()
        .timeout(time::MOCK_TIMEOUT)
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

    let config = Config::new(
        &root.display().to_string(),
        &private_key,
        "00000000000000000000000066616b65206e6574776f726b00000000deadbeef",
        &[&listen_on],
        &external_multiaddr,
        &peers_addr,
        &[&[]],
    );
    process.configure(config)?;

    let receiver = thread::spawn(move || {
        while let Ok(msg) = rx.recv() {
            let _ = msg;
        }
        log::info!("join receiver");
    });

    thread::sleep(time::INITIALIZATION);

    let before = (
        process.list_peers()?.unwrap_or_default(),
        SystemTime::now(),
    );

    let url = registry.join("split")?;
    client.get(url).send()?.status();

    // wait the libp2p notice the split
    thread::sleep(time::SPLIT);

    let after_split = (
        process.list_peers()?.unwrap_or_default(),
        SystemTime::now(),
    );

    // wait the libp2p notice the connectivity is restored and reconnect
    thread::sleep(time::REUNITE);

    let after_reunite = (
        process.list_peers()?.unwrap_or_default(),
        SystemTime::now(),
    );

    process.stop_receiving();
    receiver.join().unwrap();

    Ok(MockSplitReport {
        before,
        after_split,
        after_reunite,
    })
}
