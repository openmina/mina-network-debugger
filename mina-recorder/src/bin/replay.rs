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

    let mut recorder = P2pRecorder::new(db, false);
    let metadata = EventMetadata::default();
    recorder.on_alias(metadata.id.pid, "mainnet-node".to_owned());
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
                better_time: header.time,
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
