use std::env::args;

use mina_recorder::yamux::Header;

fn main() {
    let bytes = hex::decode(args().nth(1).unwrap()).unwrap();
    let header = Header::try_from(<[u8; 12]>::try_from(bytes.as_slice()).unwrap()).unwrap();
    println!("{header:?}");
}
