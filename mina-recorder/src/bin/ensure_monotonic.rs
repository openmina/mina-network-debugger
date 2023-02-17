use std::time::{SystemTime, Duration};

use mina_recorder::database::{FullMessage, ConnectionId};

use reqwest::blocking::Client;

fn main() {
    let client = Client::builder().build().unwrap();
    let url = "http://1.k8.openmina.com:30675/messages?limit=1000";
    let mut id = 0;
    let mut last = None::<(SystemTime, ConnectionId, bool)>;
    let mut max_difference = Duration::ZERO;
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
            if let Some((last_ts, last_cn, last_incoming)) = &last {
                if msg.timestamp < *last_ts {
                    let diff = last_ts.duration_since(msg.timestamp).unwrap();
                    if diff > max_difference {
                        max_difference = diff;
                    }
                    println!(
                        "message id: {id}, difference: {diff:?}, last ({} {}), this ({} {})",
                        last_cn, last_incoming, msg.connection_id, msg.incoming,
                    );
                    if *last_cn == msg.connection_id && *last_incoming == msg.incoming {
                        println!("error, unordered message from the same connection");
                    }
                }
            }
            last = Some((msg.timestamp, msg.connection_id, msg.incoming));
        }

        id += page.len();
    }

    println!("total messages: {id}, max difference: {max_difference:?}");
}
