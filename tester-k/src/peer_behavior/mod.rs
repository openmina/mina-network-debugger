use std::time::SystemTime;

use serde::Deserialize;

use crate::message::{DbTestReport, DbTestTimeGroupReport, DbTestTimestampsReport, DbTestEventsReport, DbEventWithMetadata};

pub fn test_database(started: SystemTime, events: Vec<DbEventWithMetadata>) -> DbTestReport {
    let timestamps = test_messages_timestamps(started);
    let events = test_events(events);

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
            log::info!("{res}");
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
        report.ordered &= group_report.ordered;

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

pub fn test_events(events: Vec<DbEventWithMetadata>) -> DbTestEventsReport {
    fn get_events() -> Vec<DbEventWithMetadata> {
        let res = reqwest::blocking::get(&format!("http://localhost:8000/libp2p_ipc/block/all"))
            .unwrap()
            .text()
            .unwrap();
        serde_json::from_str(&res).unwrap()
    }

    let mut matching = true;

    let debugger_events = get_events();
    if events.len() == debugger_events.len() {
        for i in 0..events.len() {
            let DbEventWithMetadata { events, .. } = &events[i];
            let DbEventWithMetadata { events: debugger_events, .. } = &debugger_events[i];
            matching &= events.eq(debugger_events);
        }
    } else {
        matching = false;
    }

    DbTestEventsReport {
        matching,
        events,
        debugger_events,
    }
}
