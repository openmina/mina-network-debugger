use std::time::SystemTime;

use serde::Deserialize;

use crate::message::{DbTestReport, DbTestTimeGroupReport};

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

pub fn test_database(started: SystemTime) -> DbTestReport {
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
    let mut report = DbTestReport {
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

// #[tokio::test]
// async fn check_messages() {
//     use tezedge_recorder::common::{MessageCategory, MessageKind};

//     let items = get_p2p("limit=100", "initiator").await.unwrap();

//     let expected = [
//         (0, MessageCategory::Connection, None, false),
//         (1, MessageCategory::Connection, None, true),
//         (2, MessageCategory::Meta, None, false),
//         (3, MessageCategory::Meta, None, true),
//         (4, MessageCategory::Ack, None, false),
//         (5, MessageCategory::Ack, None, true),
//         (6, MessageCategory::P2p, Some(MessageKind::Operation), false),
//     ];

//     for (id, category, kind, incoming) in &expected {
//         let inc = if *incoming { "incoming" } else { "outgoing" };
//         items.iter()
//             .find(|msg| {
//                 msg.id == *id &&
//                     msg.category.eq(category) &&
//                     msg.kind.eq(kind) &&
//                     msg.incoming == *incoming
//             })
//             .expect(&format!("not found an {} message {:?} {:?}", inc, category, kind));
//         println!("found an {} message {:?} {:?}", inc, category, kind);
//     }
// }

// #[tokio::test]
// async fn wait() {
//     let mut t = 0u8;

//     // timeout * duration = 4 minutes
//     let timeout = 24u8;
//     let duration = Duration::from_secs(10);

//     while t < timeout {
//         let response = get_p2p("limit=1000", "tezedge")
//             .await.unwrap();
//         if response.len() == 1000 {
//             break;
//         } else {
//             tokio::time::sleep(duration).await;
//             t += 1;
//         }
//     }
//     assert!(t < timeout);
// }

// #[tokio::test]
// async fn p2p_limit() {
//     for limit in 0..8 {
//         let response = get_p2p(&format!("limit={}", limit), "tezedge")
//             .await.unwrap();
//         assert_eq!(response.len(), limit);
//     }
// }

// #[tokio::test]
// async fn p2p_cursor() {
//     for cursor in 0..8 {
//         let response = get_p2p(&format!("cursor={}", cursor), "tezedge")
//             .await.unwrap();
//         assert_eq!(response[0].id, cursor);
//     }
// }

// #[tokio::test]
// async fn p2p_types_filter() {
//     let mut types = [
//         ("connection_message", 0),
//         ("metadata", 0),
//         ("advertise", 0),
//         ("get_block_headers", 0),
//         ("block_header", 0),
//     ];
//     for &mut (ty, ref mut number) in &mut types {
//         let response = get_p2p(&format!("cursor=999&limit=1000&types={}", ty), "tezedge")
//             .await.unwrap();
//         *number = response.len();
//     }

//     // for all type combination
//     for i in 0..(types.len() - 1) {
//         let &(ty_i, n_ty_i) = &types[i];
//         for j in (i + 1)..types.len() {
//             let &(ty_j, n_ty_j) = &types[j];
//             let response = get_p2p(&format!("cursor=999&limit=1000&types={},{}", ty_i, ty_j), "tezedge")
//                 .await.unwrap();
//             assert_eq!(response.len(), n_ty_i + n_ty_j);
//         }
//     }
// }
