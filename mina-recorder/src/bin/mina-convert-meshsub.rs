use std::io::{Read, Write};

fn main() {
    use std::{env, fs::File};
    use mina_recorder::meshsub;

    let arg = env::args().nth(1).unwrap();
    let mut file = File::open(&arg).unwrap();
    let mut bytes = vec![];
    file.read_to_end(&mut bytes).unwrap();
    for (n, msg) in meshsub::parse_protobuf_publish(&bytes).unwrap().enumerate() {
        let filename = arg.trim_end_matches(".bin");
        let filename = format!("{filename}_{n}.bin");
        let mut file = File::create(filename).unwrap();
        file.write_all(&msg).unwrap();
    }
}
