// fn collect_blocks() {
//     use std::{fs::File, path::PathBuf, io};

//     use mina_recorder::database::FullMessage;
//     use reqwest::{blocking::ClientBuilder, Url};

//     let path = PathBuf::from("target/state");
//     let metadata = File::open(path.join("metadata.json")).unwrap();
//     let table = serde_json::from_reader::<_, Vec<(u64, FullMessage)>>(metadata).unwrap();

//     let client = ClientBuilder::new().build().unwrap();
//     let url = "http://develop.dev.openmina.com/message_bin".parse::<Url>().unwrap();

//     for (id, _) in &table[..5] {
//         let id = id.to_string();
//         let mut response = client.get(dbg!(url.join(&id).unwrap())).send().unwrap();
//         dbg!(response.status());
//         dbg!(response.content_length());
//         let mut f = File::create(path.join(&id)).unwrap();
//         io::copy(&mut response, &mut f).unwrap();
//         dbg!(id);
//     }
// }

fn main() {
    // collect_blocks();
}
