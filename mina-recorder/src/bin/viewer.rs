use std::{env, fs, io::Read};
use mina_recorder::ChunkHeader;
use radiation::AbsorbExt;

fn main() {
    let filename = env::args().nth(1).unwrap();
    let mut bytes = Vec::new();
    fs::File::open(filename).unwrap().read_to_end(&mut bytes).unwrap();
    let mut offset = 0;
    while offset < bytes.len() {
        let header = ChunkHeader::absorb_ext(&bytes[offset..(offset + ChunkHeader::SIZE)]).unwrap();
        let data_offset = offset + ChunkHeader::SIZE;
        offset = data_offset + (header.size as usize);
        println!("{header} {}", hex::encode(&bytes[data_offset..offset]));
    }
}
