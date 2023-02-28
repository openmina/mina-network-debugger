use std::{env, fs, path::Path, thread, time::{Duration, SystemTime}, net::IpAddr};

use reqwest::blocking::ClientBuilder;

use mina_ipc::message::{outgoing, Config, ConsensusMessage};

use crate::message::{DbEvent, DbEventWithMetadata, GossipNetMessageV2Short};

use super::{
    constants::*,
    libp2p_helper::Process,
    message::{Registered, Report},
    tcpflow::TcpFlow,
    peer_behavior,
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
    let request = match client.get(url).send() {
        Ok(v) => v,
        Err(err) => {
            log::error!("{err}");
            return Ok(());
        }
    };
    let response = serde_json::from_reader::<_, Registered>(request)?;
    let this_addr = response.external;

    let root = AsRef::<Path>::as_ref("/root/.mina-test");
    fs::create_dir_all(root).unwrap();

    let listen_on = format!("/ip4/0.0.0.0/tcp/{PEER_PORT}");
    let private_key = hex::decode(response.secret_key).unwrap();
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
    let (mut process, rx) = Process::spawn();
    process.configure(config)?;
    process.subscribe(0, topic)?;

    let stream_sender = if let Some((_, peer_id)) = response.peers.last() {
        if env::var("LAUNCH_STREAM_SENDER").is_ok() {
            launch_stream_sender(peer_id, blocks * delay, &mut process)?
        } else {
            None
        }
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
                    peer_id,
                    peer_host,
                    peer_port,
                    data,
                } => {
                    match ConsensusMessage::from_bytes(&data).unwrap() {
                        ConsensusMessage::Test(msg) => {
                            log::info!(
                                "worker {this_addr} received from {peer_id} {peer_host}:{peer_port}, msg: {msg}"
                            );
                            let parse_block_height = |s: &str| s.split("slot: ").nth(1)?.parse().ok();
                            events.push(DbEventWithMetadata {
                                time_microseconds: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("msg").as_micros() as u64,
                                events: vec![DbEvent::ReceivedGossip {
                                    peer_id,
                                    peer_address: format!("{peer_host}:{peer_port}"),
                                    msg: GossipNetMessageV2Short::TestMessage {
                                        height: parse_block_height(&msg).unwrap_or(u32::MAX),
                                    },
                                    hash: hex::encode(ConsensusMessage::calc_hash(&data, &topic_owned)),
                                }],
                            });
                        }
                        msg => log::info!("worker {this_addr} received unexpected {msg:?}"),
                    }
                }
                outgoing::PushMessage::IncomingStream { peer_id, peer_host, peer_port, protocol, stream_id } => {
                    log::info!("worker {this_addr} received from {peer_id} {peer_host}:{peer_port}, new stream {stream_id} {protocol}");
                }
                outgoing::PushMessage::StreamMessageReceived { data, stream_id } => {
                    let value = u32::from_ne_bytes(data[..4].try_into().unwrap());
                    log::info!("worker {this_addr} received at stream {stream_id}, msg {value}");
                }
                msg => log::info!("worker {this_addr} received unexpected {msg:?}"),
            }
        }
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
                time_microseconds: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("msg").as_micros() as u64,
                events: vec![DbEvent::PublishGossip {
                    msg: GossipNetMessageV2Short::TestMessage {
                        height: slot,
                    },
                    hash: hex::encode(ConsensusMessage::calc_hash(&data, topic)),
                }],
            });
            process
                .publish(topic.to_string(), data)
                .unwrap();
        }
    }

    if let Some(stream_sender) = stream_sender {
        stream_sender.join().unwrap();
    }

    let (ipc, _status_code) = process.stop().expect("can check debuggers output");
    let recv_events = receiver.join().unwrap();

    let mut events = sent_events;
    events.extend_from_slice(&recv_events);
    events
        .sort_by(|a, b| {
            a.height().cmp(&b.height()).then(a.time_microseconds.cmp(&b.time_microseconds))
        });
    let db_test = peer_behavior::test_database(started, events, response.info.peer_id);

    let summary_json = serde_json::to_string(&Report { ipc, db_test })?;
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

    Ok(())
}

fn launch_stream_sender(peer_id: &str, seconds: u32, process: &mut Process) -> mina_ipc::Result<Option<thread::JoinHandle<()>>> {
    let mut tries = 5;
    let sender = loop {
        log::info!("try open stream");
        match process.open_stream(peer_id, "/mina/peer-exchange")? {
            Ok(v) => break Some(v),
            Err(err) => {
                log::warn!("failed to open stream {err}");
                tries -= 1;
                if tries == 0 {
                    break None;
                }
            },
        }
        thread::sleep(Duration::from_secs(1));
    };
    if let Some(mut sender) = sender {
        let handle = thread::spawn(move || {
            for millisecond in 0..(1000 * seconds) {
                let instant = std::time::Instant::now();

                let mut data = [0; 1024];
                for i in 0..128 {
                    data[(4 * i)..(4 * (i + 1))].clone_from_slice(&millisecond.to_ne_bytes());
                }

                sender.send_stream(&data).unwrap();
                let elapsed = instant.elapsed();
                if elapsed < Duration::from_millis(1) {
                    thread::sleep(Duration::from_millis(1) - elapsed);
                }
            }
        });
        Ok(Some(handle))
    } else {
        Ok(None)
    }
}
