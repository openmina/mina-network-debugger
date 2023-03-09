use std::{
    time::{SystemTime, Duration},
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
};

use reqwest::blocking::{ClientBuilder, Client};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct TestReport {
    pub timestamps: DbTestTimestampsReport,
    pub events: DbTestEventsReport,
    pub order: DbTestOrderReport,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DbTestTimestampsReport {
    pub start: SystemTime,
    pub end: SystemTime,
    pub group_report: Vec<DbTestTimeGroupReport>,
    pub total_messages: usize,
    pub ordered: bool,
    pub timestamps_filter_ok: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DbTestTimeGroupReport {
    pub timestamps: Vec<SystemTime>,
    pub timestamps_filter_ok: bool,
    pub ordered: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DbTestEventsReport {
    pub matching: bool,
    pub consistent: bool,
    pub events: Vec<DbEventWithMetadata>,
    pub debugger_events: Vec<DbEventWithMetadata>,
    pub network_events: BTreeMap<u32, Vec<BlockNetworkEvent>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlockNetworkEvent {
    pub producer_id: String,
    pub hash: String,
    pub block_height: u32,
    pub global_slot: u32,
    pub incoming: bool,
    pub time: SystemTime,
    pub better_time: SystemTime,
    pub latency: Option<Duration>,
    pub sender_addr: SocketAddr,
    pub receiver_addr: SocketAddr,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DbTestOrderReport {
    pub messages: usize,
    pub unordered_num: Vec<(u64, bool, u32, u32)>,
    pub unordered_time: Vec<(u64, bool, SystemTime, SystemTime)>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DbEventWithMetadata {
    pub time_microseconds: u64,
    pub events: Vec<DbEvent>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum GossipNetMessageV2Short {
    TestMessage { height: u32 },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum DbEvent {
    ReceivedGossip {
        peer_id: String,
        peer_address: String,
        msg: GossipNetMessageV2Short,
        hash: String,
    },
    PublishGossip {
        msg: GossipNetMessageV2Short,
        hash: String,
    },
}

impl DbEventWithMetadata {
    pub fn height(&self) -> u32 {
        match self.events.first() {
            Some(DbEvent::PublishGossip {
                msg: GossipNetMessageV2Short::TestMessage { height },
                ..
            }) => *height,
            Some(DbEvent::ReceivedGossip {
                msg: GossipNetMessageV2Short::TestMessage { height },
                ..
            }) => *height,
            None => u32::MAX,
        }
    }
}

pub fn run(started: SystemTime, events: Vec<DbEventWithMetadata>, peer_id: String) -> TestReport {
    let client = ClientBuilder::new().build().unwrap();

    let timestamps = test_messages_timestamps(&client, started);
    let events = test_events(events, peer_id);
    let order = test_order(&client);

    TestReport {
        timestamps,
        events,
        order,
    }
}

#[derive(Deserialize, Clone, PartialEq, Eq)]
pub struct FullMessage {
    pub connection_id: u64,
    pub remote_addr: String,
    pub incoming: bool,
    pub timestamp: SystemTime,
    pub stream_id: StreamId,
    pub stream_kind: String,
    pub message: serde_json::Value,
    pub size: u32,
}

#[derive(Deserialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum StreamId {
    Handshake,
    Forward(u64),
    Backward(u64),
}

fn get_messages(client: &Client, params: &str) -> anyhow::Result<Vec<(u64, FullMessage)>> {
    let time = |t: &SystemTime| {
        t.duration_since(SystemTime::UNIX_EPOCH)
            .expect("cannot fail")
            .as_secs_f64()
    };

    let res = client
        .get(&format!("http://localhost:8000/messages?{params}"))
        .send()?
        .text()?;
    if let Ok(msgs) = serde_json::from_str::<Vec<(u64, FullMessage)>>(&res) {
        Ok(msgs)
    } else {
        let msgs = serde_json::from_str::<Vec<FullMessage>>(&res)?
            .into_iter()
            .map(|m| (time(&m.timestamp) as u64, m))
            .collect();
        Ok(msgs)
    }
}

fn get_message(client: &Client, id: u64) -> anyhow::Result<Vec<u8>> {
    client
        .get(&format!("http://localhost:8000/message_bin/{id}"))
        .send()?
        .bytes()
        .map(|x| x.to_vec())
        .map_err(Into::into)
}

pub fn test_messages_timestamps(client: &Client, started: SystemTime) -> DbTestTimestampsReport {
    // timestamp
    let time = |t: &SystemTime| {
        t.duration_since(SystemTime::UNIX_EPOCH)
            .expect("cannot fail")
            .as_secs_f64()
    };

    const GROUPS: u64 = 10;
    let mut report = DbTestTimestampsReport {
        start: started,
        end: SystemTime::now(),
        group_report: vec![],
        total_messages: 0,
        ordered: true,
        timestamps_filter_ok: true,
    };
    let s = time(&started);
    let duration = report
        .end
        .duration_since(started)
        .expect("system time changed during test")
        .as_secs_f64();

    for i in 0..GROUPS {
        let start = (s + i as f64 * duration / (GROUPS as f64)) as u64;
        let end = (s + (i + 1) as f64 * duration / (GROUPS as f64)) as u64;
        let messages =
            match get_messages(&client, &format!("timestamp={start}&limit_timestamp={end}")) {
                Ok(v) => v,
                Err(err) => {
                    log::error!("{err}");
                    vec![]
                }
            };
        report.total_messages += messages.len();
        let mut group_report = DbTestTimeGroupReport {
            timestamps: messages.into_iter().map(|(_, m)| m.timestamp).collect(),
            timestamps_filter_ok: true,
            ordered: true,
        };
        let mut message_sorted = group_report.timestamps.clone();
        message_sorted.sort();

        group_report.ordered = message_sorted.eq(&group_report.timestamps);
        // WARNING: skip order test, uncomment it when will be merged commit that removing db ids
        // report.ordered &= group_report.ordered;

        if !message_sorted.is_empty() {
            let first = message_sorted.first().expect("checked above");
            group_report.timestamps_filter_ok = time(first) >= start as f64;
            let last = message_sorted.last().expect("checked above");
            group_report.timestamps_filter_ok = time(last) <= end as f64;
        }
        report.timestamps_filter_ok &= group_report.timestamps_filter_ok;

        report.group_report.push(group_report);
    }

    report
}

pub fn test_order(client: &Client) -> DbTestOrderReport {
    let mut report = DbTestOrderReport {
        messages: 0,
        unordered_num: vec![],
        unordered_time: vec![],
    };

    let mut messages = vec![];
    let mut id = 0;
    loop {
        let params = format!("stream_kind=/mina/node-status&limit=500&id={id}");
        let mut v = match get_messages(&client, &params) {
            Ok(v) => v,
            Err(err) => {
                log::error!("{err}");
                vec![]
            }
        };
        if let Some((last, _)) = v.last() {
            id = *last + 1;
        } else {
            break;
        }
        messages.append(&mut v);
    }

    let mut last_incoming = None;
    let mut n_incoming = 0;
    let mut last_outgoing = None;
    let mut n_outgoing = 0;
    for (id, msg) in messages {
        // assert!(msg.size % 0x400 == 0);
        // assert_eq!(msg.stream_kind, "/mina/node-status");
        let bytes = get_message(&client, id).unwrap();
        for this_n in bytes
            .chunks(0x400)
            .map(|c| u32::from_ne_bytes(c[..4].try_into().unwrap()))
        {
            report.messages += 1;
            let (last, last_n) = if msg.incoming {
                (&mut last_incoming, &mut n_incoming)
            } else {
                (&mut last_outgoing, &mut n_outgoing)
            };
            if let Some(last) = last {
                if *last_n + 1 != this_n {
                    log::error!("{this_n} after {last_n}");
                    report
                        .unordered_num
                        .push((id, msg.incoming, *last_n, this_n));
                }
                if *last > msg.timestamp {
                    log::error!("{:?} after {last:?}", msg.timestamp);
                    report
                        .unordered_time
                        .push((id, msg.incoming, *last, msg.timestamp));
                }
            } else {
                if *last_n != 0 && this_n != 0 {
                    log::error!("{last_n} and {this_n} at beginning");
                }
            }
            *last = Some(msg.timestamp);
            *last_n = this_n;
        }
    }

    report
}

pub fn test_events(events: Vec<DbEventWithMetadata>, peer_id: String) -> DbTestEventsReport {
    #[derive(Clone, Deserialize)]
    pub struct BlockStat {
        pub height: u32,
        pub events: Vec<BlockNetworkEvent>,
    }

    fn get_events() -> Vec<DbEventWithMetadata> {
        let res = reqwest::blocking::get(&format!("http://localhost:8000/libp2p_ipc/block/all"))
            .unwrap()
            .text()
            .unwrap();
        serde_json::from_str(&res).unwrap()
    }

    fn get_network_event(height: u32) -> Vec<BlockNetworkEvent> {
        let res = reqwest::blocking::get(&format!("http://localhost:8000/block/{height}"))
            .unwrap()
            .text()
            .unwrap();
        let BlockStat { events, .. } = serde_json::from_str(&res).unwrap();
        events
    }

    let mut matching = true;

    let debugger_events = get_events();
    if events.len() <= debugger_events.len() {
        // let events_set = events.iter().flat_map(|x| &x.events).cloned().collect::<BTreeSet<_>>();
        // let debugger_events_set = debugger_events.iter().flat_map(|x| &x.events).cloned().collect::<BTreeSet<_>>();
        // matching &= events_set.eq(&debugger_events_set);
        for i in 0..events.len() {
            let DbEventWithMetadata { events, .. } = &events[i];
            let DbEventWithMetadata {
                events: debugger_events,
                ..
            } = &debugger_events[i];
            matching &= events.eq(debugger_events);
        }
    } else {
        matching = false;
    }

    let mut consistent = true;
    let mut network_events = BTreeMap::new();
    let heights = debugger_events
        .iter()
        .map(|x| x.height())
        .collect::<BTreeSet<_>>();
    for height in heights {
        let network_e = get_network_event(height);
        for event in debugger_events.iter().filter(|x| x.height() == height) {
            let _time = event.time_microseconds;
            let event = match event.events.first() {
                Some(v) => v,
                None => continue,
            };

            // for each ipc event must exist network event
            match event {
                DbEvent::ReceivedGossip {
                    peer_id,
                    peer_address,
                    hash,
                    ..
                } => {
                    let _ = peer_id;
                    let exist = network_e
                        .iter()
                        .find(|e| {
                            e.incoming
                                && e.block_height == height
                                && peer_address.starts_with(&e.sender_addr.ip().to_string())
                                && e.hash.eq(hash)
                        })
                        .is_some();
                    consistent &= exist;
                }
                DbEvent::PublishGossip { hash, .. } => {
                    let exist = network_e
                        .iter()
                        .find(|e| {
                            !e.incoming
                                && e.producer_id == peer_id
                                && e.block_height == height
                                && e.hash.eq(hash)
                        })
                        .is_some();
                    let _ = exist;
                    // consistent &= exist;
                    // at the end of the test, mock may publish block,
                    // but it will have no time to be broadcasted, so debugger records nothing
                }
            }
        }
        network_events.insert(height, network_e);
    }

    DbTestEventsReport {
        matching,
        consistent,
        events,
        debugger_events,
        network_events,
    }
}
