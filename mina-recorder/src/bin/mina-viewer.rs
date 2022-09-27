use std::{env, fs, io::Read};
use mina_recorder::ChunkHeader;
use radiation::AbsorbExt;

fn main() {
    let filename = env::args().nth(1).unwrap();

    // group together all messages in row from the same party
    let glue = env::args().nth(2).is_some();

    let mut bytes = Vec::new();
    fs::File::open(filename)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let mut offset = 0;
    let mut prev = None;
    while offset < bytes.len() {
        let header = ChunkHeader::absorb_ext(&bytes[offset..(offset + ChunkHeader::SIZE)]).unwrap();
        let data_offset = offset + ChunkHeader::SIZE;
        offset = data_offset + (header.size as usize);
        if glue {
            if prev.is_none() || prev.unwrap_or(false) != header.incoming {
                if prev.is_some() {
                    println!();
                }
                print!("{header} ");
                prev = Some(header.incoming);
            }
            print!("{}", hex::encode(&bytes[data_offset..offset]));
            if &bytes[data_offset..(data_offset + 2)] == b"\x00\x20" {
                break;
            }
        } else {
            println!("{header} {}", hex::encode(&bytes[data_offset..offset]));
        }
    }
    println!();
}
