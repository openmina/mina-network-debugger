use std::{
    net::IpAddr,
    time::{Duration, SystemTime},
    path::Path,
    fs, thread,
    sync::mpsc,
};

use mina_ipc::message::{
    Config,
    outgoing::{self, PushMessage},
    ConsensusMessage,
};
use reqwest::{
    Url,
    blocking::{ClientBuilder, Client},
};

use crate::registry::{
    server,
    messages::{Registered, MockReport},
};
use crate::libp2p_helper::Process;

use super::{
    tcpflow::TcpFlow,
    tests::{self, TestReport, DbEventWithMetadata, GossipNetMessageV2Short, DbEvent},
};

pub const PEER_PORT: u16 = 8302;

pub fn run(
    blocks: u32,
    delay: u32,
    registry: &str,
    build_number: u32,
    this_ip: IpAddr,
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

    // spawn tcpflow to observe the network
    let network_observer = match TcpFlow::spawn(this_ip) {
        Ok(v) => v,
        Err(err) => {
            process.stop()?;
            return Err(err.into());
        }
    };

    let registry = format!("http://{registry}:{}", server::PORT)
        .parse::<Url>()
        .expect("hostname must be valid");

    let result = run_inner(
        blocks,
        delay,
        build_number,
        &client,
        &registry,
        &mut process,
        rx,
    );
    // ensure everything is stopped regardless `run_inner` succeeded or not
    let (ipc, _status_code) = process.stop().expect("can check debuggers output");
    let net_report = network_observer.stop();

    if let Some(net_report) = net_report {
        let net_json = serde_json::to_string(&net_report)?;
        let url = registry.join(&format!("net_report?build_number={build_number}"))?;
        // TODO: check
        let _status = client.post(url).body(net_json).send()?.status();
    }

    let test = result?;
    let mock_json = serde_json::to_string(&MockReport { ipc, test })?;
    let url = registry.join(&format!("mock_report?build_number={build_number}"))?;
    // TODO: check
    let _status = client.post(url).body(mock_json).send()?.status();

    Ok(())
}

fn run_inner(
    blocks: u32,
    delay: u32,
    build_number: u32,
    client: &Client,
    registry: &Url,
    process: &mut Process,
    rx: mpsc::Receiver<PushMessage>,
) -> anyhow::Result<TestReport> {
    // register on the registry
    let url = registry.join(&format!("register?build_number={build_number}"))?;
    let response = serde_json::from_reader::<_, Registered>(client.get(url).send()?)?;

    let this_addr = response.external;

    let root = AsRef::<Path>::as_ref("/root/.mina-test");
    fs::create_dir_all(root)?;

    let listen_on = format!("/ip4/0.0.0.0/tcp/{PEER_PORT}");
    let private_key = hex::decode(response.secret_key)?;
    let peers_addr = response
        .peers
        .iter()
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
    process.configure(config)?;
    process.subscribe(0, topic)?;

    let prev_peer = response.prev.and_then(|ip| response.peers.get(&ip));
    let stream_sender = if let Some(peer_id) = prev_peer {
        launch_stream_sender(peer_id, 2, process)?
    } else {
        None
    };

    let topic_owned = topic.to_owned();
    let receiver = thread::spawn(move || {
        let mut events = vec![];
        while let Ok(msg) = rx.recv() {
            match msg {
                outgoing::PushMessage::GossipReceived {
                    subscription_id: 0,
                    peer: outgoing::Peer {
                        id,
                        host,
                        port,
                    },
                    data,
                } => match ConsensusMessage::from_bytes(&data).unwrap() {
                    ConsensusMessage::Test(msg) => {
                        log::info!(
                            "worker {this_addr} received from {id} {host}:{port}, msg: {msg}"
                        );
                        let parse_block_height = |s: &str| s.split("slot: ").nth(1)?.parse().ok();
                        events.push(DbEventWithMetadata {
                            time_microseconds: SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .expect("msg")
                                .as_micros() as u64,
                            events: vec![DbEvent::ReceivedGossip {
                                peer_id: id,
                                peer_address: format!("{host}:{port}"),
                                msg: GossipNetMessageV2Short::TestMessage {
                                    height: parse_block_height(&msg).unwrap_or(u32::MAX),
                                },
                                hash: hex::encode(ConsensusMessage::calc_hash(&data, &topic_owned)),
                            }],
                        });
                    }
                    msg => log::info!("worker {this_addr} received unexpected {msg:?}"),
                },
                outgoing::PushMessage::IncomingStream {
                    peer_id,
                    peer_host,
                    peer_port,
                    protocol,
                    stream_id,
                } => {
                    log::info!("worker {this_addr} received from {peer_id} {peer_host}:{peer_port}, new stream {stream_id} {protocol}");
                }
                outgoing::PushMessage::StreamMessageReceived { data, stream_id } => {
                    let value = u32::from_ne_bytes(data[..4].try_into().expect("cannot fail"));
                    log::debug!("worker {this_addr} received at stream {stream_id}, {value}");
                }
                msg => log::info!("worker {this_addr} received unexpected {msg:?}"),
            }
        }
        log::info!("join receiver");
        events
    });

    let started = SystemTime::now();
    let mut sent_events = vec![];
    for slot in 0..blocks {
        thread::sleep(Duration::from_secs(delay as u64));
        // chance is 1 per nodes number
        if rand::random::<usize>() < usize::MAX / ((20.0 as f32).sqrt() as usize) {
            let msg_str = format!("test message, id: {this_addr}, slot: {slot}");
            log::info!("worker {this_addr} publish msg: {msg_str}");
            let msg = ConsensusMessage::Test(msg_str);
            let data = msg.into_bytes();
            sent_events.push(DbEventWithMetadata {
                time_microseconds: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("cannot fail")
                    .as_micros() as u64,
                events: vec![DbEvent::PublishGossip {
                    msg: GossipNetMessageV2Short::TestMessage { height: slot },
                    hash: hex::encode(ConsensusMessage::calc_hash(&data, topic)),
                }],
            });
            process.publish(topic.to_string(), data).unwrap();
        }
    }

    if let Some(stream_sender) = stream_sender {
        stream_sender.join().unwrap();
    }

    process.stop_receiving();
    let recv_events = receiver.join().unwrap();

    let mut events = sent_events;
    events.extend_from_slice(&recv_events);
    events.sort_by(|a, b| {
        a.height()
            .cmp(&b.height())
            .then(a.time_microseconds.cmp(&b.time_microseconds))
    });

    Ok(tests::run(started, events, response.info.peer_id))
}

fn launch_stream_sender(
    peer_id: &str,
    seconds: u32,
    process: &mut Process,
) -> mina_ipc::Result<Option<thread::JoinHandle<()>>> {
    let mut tries = 10;
    let sender = loop {
        log::info!("try open stream");
        match process.open_stream(peer_id, "/mina/node-status")? {
            Ok(v) => break Some(v),
            Err(err) => {
                log::warn!("failed to open stream {err}");
                tries -= 1;
                if tries == 0 {
                    break None;
                }
            }
        }
        thread::sleep(Duration::from_secs(1));
    };
    if let Some(mut sender) = sender {
        let handle = thread::spawn(move || {
            for millisecond in 0..(100 * seconds) {
                let instant = std::time::Instant::now();

                let mut data = [0; 1024];
                for i in 0..128 {
                    data[(4 * i)..(4 * (i + 1))].clone_from_slice(&millisecond.to_ne_bytes());
                }

                sender.send_stream(&data).unwrap();
                let elapsed = instant.elapsed();
                if elapsed < Duration::from_millis(10) {
                    thread::sleep(Duration::from_millis(10) - elapsed);
                }
            }
        });
        Ok(Some(handle))
    } else {
        Ok(None)
    }
}
