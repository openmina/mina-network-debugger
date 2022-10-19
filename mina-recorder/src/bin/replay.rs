use std::{env, fs, io::Read, time::Duration};

use mina_recorder::{
    P2pRecorder, database::DbFacade, EventMetadata, ChunkHeader, ConnectionInfo, EncryptionStatus,
};
use radiation::AbsorbExt;

fn main() {
    let filename = env::args()
        .nth(1)
        .expect("connection dump file: `/streams/connection00004d8b");

    let db = DbFacade::open("target/replay_db").unwrap();
    const MAINNET_CHAIN: &str =
        "/coda/0.0.1/5f704cc0c82e0ed70e873f0893d7e06f148524e3f0bdae2afb02e7819a0c24d1";

    let mut recorder = P2pRecorder::new(db, MAINNET_CHAIN.to_string(), false);
    let metadata = EventMetadata::default();
    recorder.on_connect(true, metadata, 0);

    let mut bytes = Vec::new();
    fs::File::open(filename)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();

    let mut offset = 0;
    while offset < bytes.len() {
        let header = ChunkHeader::absorb_ext(&bytes[offset..(offset + ChunkHeader::SIZE)]).unwrap();
        let data_offset = offset + ChunkHeader::SIZE;
        offset = data_offset + (header.size as usize);
        if let EncryptionStatus::Raw = &header.encryption_status {
            let metadata = EventMetadata {
                id: ConnectionInfo::default(),
                time: header.time,
                duration: Duration::from_secs(0),
            };
            recorder.on_data(
                header.incoming,
                metadata,
                0,
                bytes[data_offset..offset].to_vec(),
            );
        }
    }
}
