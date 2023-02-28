use std::{time::SystemTime, collections::{BTreeSet, BTreeMap}};

use serde::Deserialize;

use crate::message::{DbTestReport, DbTestTimeGroupReport, DbTestTimestampsReport, DbTestEventsReport, DbEventWithMetadata, DbEvent, BlockNetworkEvent};

pub fn test_database(started: SystemTime, events: Vec<DbEventWithMetadata>, peer_id: String) -> DbTestReport {
    let timestamps = test_messages_timestamps(started);
    let events = test_events(events, peer_id);

    DbTestReport {
        timestamps,
        events,
    }
}

pub fn test_messages_timestamps(started: SystemTime) -> DbTestTimestampsReport {
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

    fn get_messages(params: &str) -> Vec<FullMessage> {
        let res = reqwest::blocking::get(&format!("http://localhost:8000/messages?{params}"))
            .unwrap()
            .text()
            .unwrap();
        if let Ok(msgs) = serde_json::from_str::<Vec<(u64, FullMessage)>>(&res) {
            msgs.into_iter().map(|(_, m)| m).collect()
        } else {
            serde_json::from_str(&res).unwrap()
        }
    }

    // timestamp
    let time = |t: &SystemTime| t.duration_since(SystemTime::UNIX_EPOCH).expect("cannot fail").as_secs_f64();

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
    let duration = report.end.duration_since(started).expect("system time changed during test").as_secs_f64();

    for i in 0..GROUPS {
        let start = (s + i as f64 * duration / (GROUPS as f64)) as u64;
        let end = (s + (i + 1) as f64 * duration / (GROUPS as f64)) as u64;
        let messages = get_messages(&format!("timestamp={start}&limit_timestamp={end}"));
        report.total_messages += messages.len();
        let mut group_report = DbTestTimeGroupReport {
            timestamps: messages.into_iter().map(|m| m.timestamp).collect(),
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

    let _messages = get_messages("stream_kind=/mina/peer-exchange");

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
    if events.len() == debugger_events.len() {
        // let events_set = events.iter().flat_map(|x| &x.events).cloned().collect::<BTreeSet<_>>();
        // let debugger_events_set = debugger_events.iter().flat_map(|x| &x.events).cloned().collect::<BTreeSet<_>>();
        // matching &= events_set.eq(&debugger_events_set);
        for i in 0..events.len() {
            let DbEventWithMetadata { events, .. } = &events[i];
            let DbEventWithMetadata { events: debugger_events, .. } = &debugger_events[i];
            matching &= events.eq(debugger_events);
        }
    } else {
        matching = false;
    }

    let mut consistent = true;
    let mut network_events = BTreeMap::new();
    let heights = debugger_events.iter().map(|x| x.height()).collect::<BTreeSet<_>>();
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
                DbEvent::ReceivedGossip { peer_id, peer_address, hash, .. } => {
                    let _ = peer_id;
                    let exist = network_e.iter()
                        .find(|e| {
                            e.incoming && e.block_height == height && peer_address.starts_with(&e.sender_addr.ip().to_string()) && e.hash.eq(hash)
                        }).is_some();
                    consistent &= exist;
                },
                DbEvent::PublishGossip { hash, .. } => {
                    let exist = network_e.iter()
                        .find(|e| {
                            !e.incoming && e.producer_id == peer_id && e.block_height == height && e.hash.eq(hash)
                        }).is_some();
                    let _ = exist;
                    // consistent &= exist;
                    // at the end of the test, mock may publish block,
                    // but it will have no time to be broadcasted, so debugger records nothing
                },
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
