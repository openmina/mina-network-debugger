use std::{env, fs};

use mina_recorder::ChunkParser;

fn main() {
    let filename = env::args().nth(1).unwrap();

    // group together all messages in row from the same party
    let glue = env::args().nth(2).is_some();

    let parser = ChunkParser::new(fs::File::open(filename).unwrap());

    let mut prev = None;
    for (header, data) in parser {
        if glue {
            if prev.is_none() || prev.unwrap_or(false) != header.incoming {
                if prev.is_some() {
                    println!();
                }
                print!("{header} ");
                prev = Some(header.incoming);
            }
            print!("{}", hex::encode(&data));
            if &data[..2] == b"\x00\x20" {
                break;
            }
        } else {
            println!("{header} {}", hex::encode(&data));
        }
    }
    println!();
}
