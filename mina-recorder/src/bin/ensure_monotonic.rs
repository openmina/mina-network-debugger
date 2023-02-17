use std::time::{SystemTime, Duration};

use mina_recorder::database::FullMessage;

use reqwest::blocking::Client;

fn main() {
    let client = Client::builder().build().unwrap();
    let url = "http://1.k8.openmina.com:30675/messages?limit=1000";
    let mut id = 0;
    let mut last = None::<SystemTime>;
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
            if let Some(last) = &last {
                if msg.timestamp < *last {
                    let diff = last.duration_since(msg.timestamp).unwrap();
                    if diff > max_difference {
                        max_difference = diff;
                    }
                    println!("message id: {id}, difference: {diff:?}");
                }
            }
            last = Some(msg.timestamp);
        }

        id += page.len();
    }

    println!("total messages: {id}, max difference: {max_difference:?}");
}
