use std::{env, fs, path::Path, thread, time::Duration, net::IpAddr};

use reqwest::blocking::ClientBuilder;

use mina_ipc::message::{outgoing, Config, ConsensusMessage};

use super::{
    constants::*,
    libp2p_helper::Process,
    message::{Registered, Report},
    tcpflow::TcpFlow,
};

pub fn run(blocks: u32, delay: u32) -> anyhow::Result<()> {
    let center_host = env::var("REGISTRY")?;
    let build_number = env::var("BUILD_NUMBER")?.parse::<u32>()?;
    let this_ip = env::var("MY_POD_IP")?.parse::<IpAddr>()?;

    let network = TcpFlow::run(this_ip)?;

    let client = ClientBuilder::new()
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = format!("http://{center_host}:{CENTER_PORT}/register?build_number={build_number}");
    let request = client.get(url).send()?;
    let response = serde_json::from_reader::<_, Registered>(request)?;
    let this_addr = response.external;

    let root = AsRef::<Path>::as_ref("/root/.mina-test");
    fs::create_dir_all(root).unwrap();

    let listen_on = format!("/ip4/0.0.0.0/tcp/{PEER_PORT}");
    let private_key = hex::decode(response.secret_key).unwrap();
    let peers_addr = response
        .peers
        .into_iter()
        .map(|(address, peer_id)| format!("/dns4/{address}/tcp/{PEER_PORT}/p2p/{peer_id}"))
        .collect::<Vec<String>>();
    let external_multiaddr = format!("/dns4/{this_addr}/tcp/{PEER_PORT}");
    let topic = "coda/test-messages/0.0.1";

    let config = Config::new(
        &root.display().to_string(),
        &private_key,
        "00000000000000000000000066616b65206e6574776f726b00000000deadbeef",
        &[&listen_on],
        &external_multiaddr,
        &peers_addr,
        &[&[topic]],
    );
    let (mut process, rx) = Process::spawn();
    process.configure(config)?;
    process.subscribe(0, topic)?;

    let topic = topic.to_owned();
    let receiver = thread::spawn(move || {
        let mut recv_cnt = 0;
        while let Ok(msg) = rx.recv() {
            match msg {
                outgoing::PushMessage::GossipReceived {
                    peer_id,
                    peer_host,
                    peer_port,
                    data,
                } => {
                    match ConsensusMessage::from_bytes(&data, &topic).unwrap() {
                        ConsensusMessage::Test(msg) => {
                            log::info!(
                                "worker {this_addr} received from {peer_id} {peer_host}:{peer_port}, msg: {msg}"
                            );
                        }
                        msg => log::info!("worker {this_addr} received unexpected {msg:?}"),
                    }
                    recv_cnt += 1;
                }
                msg => log::info!("worker {this_addr} received unexpected {msg:?}"),
            }
        }
        recv_cnt
    });

    let mut sent_cnt = 0;
    for slot in 0..blocks {
        thread::sleep(Duration::from_secs(delay as u64));
        // chance is 1 per nodes number
        if rand::random::<usize>() < usize::MAX / ((20.0 as f32).sqrt() as usize) {
            let msg_str = format!("test message, id: {this_addr}, slot: {slot}");
            log::info!("worker {this_addr} publish msg: {msg_str}");
            let msg = ConsensusMessage::Test(msg_str);
            process
                .publish("coda/test-messages/0.0.1".to_string(), msg.into_bytes())
                .unwrap();
            sent_cnt += 1;
        }
    }

    let (ipc, _status_code) = process.stop().expect("can check debuggers output");
    let recv_cnt = receiver.join().unwrap();

    let summary_json = serde_json::to_string(&Report { ipc })?;
    let url = format!("http://{center_host}:{CENTER_PORT}/report/node?build_number={build_number}");
    // TODO: check
    let _status = client.post(url).body(summary_json).send()?.status();

    if let Some(network_report) = network.stop() {
        let network_json = serde_json::to_string(&network_report)?;
        let url =
            format!("http://{center_host}:{CENTER_PORT}/net_report?build_number={build_number}");
        // TODO: check
        let _status = client.post(url).body(network_json).send()?.status();
    }

    log::info!("sent: {sent_cnt}, recv: {recv_cnt}");

    Ok(())
}
