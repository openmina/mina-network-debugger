use std::{time::SystemTime, collections::BTreeMap};

use mina_recorder::database::{FullMessage, ConnectionId};

use reqwest::blocking::Client;

fn main() {
    let client = Client::builder().build().unwrap();
    let url = "http://1.k8.openmina.com:30675/messages?limit=1000";
    let mut id = 0;
    let mut last = None::<(SystemTime, ConnectionId, bool)>;
    let mut diffs = vec![];
    loop {
        let response = client.get(format!("{url}&id={id}")).send().unwrap();
        if !response.status().is_success() {
            break;
        }
        let page = serde_json::from_reader::<_, Vec<(u64, FullMessage)>>(response).unwrap();
        if page.is_empty() {
            break;
        }
        for (id, msg) in &page {
            if let Some((last_ts, last_cn, last_incoming)) = last {
                last = Some((msg.timestamp, msg.connection_id, msg.incoming));
                let Ok(diff) = last_ts.duration_since(msg.timestamp) else {
                    continue;
                };
                if diff.is_zero() {
                    continue;
                }
                diffs.push((*id, diff));
                println!(
                    "message id: {id}, difference: {diff:?}, last ({} {}), this ({} {})",
                    last_cn, last_incoming, msg.connection_id, msg.incoming,
                );
                if last_cn == msg.connection_id && last_incoming == msg.incoming {
                    println!("error, unordered message from the same connection");
                }
            } else {
                last = Some((msg.timestamp, msg.connection_id, msg.incoming));
            }
        }

        id += page.len();
    }

    let mut counters = BTreeMap::<u32, u32>::new();
    let mut max = Default::default();
    for (_, diff) in &diffs {
        let key = diff.as_nanos().ilog2();
        *counters.entry(key).or_default() += 1;
        if max < *diff {
            max = *diff;
        }
    }

    let mut a = 0;
    for (key, counter) in counters {
        a += counter;
        println!("{a} are smaller {}", 1 << key + 1);
    }

    println!("total messages: {id}, max difference: {max:?}");
}
