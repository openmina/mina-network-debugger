use super::event::{EventMetadata, ConnectionInfo};

#[derive(Default)]
pub struct Tester {}

impl Tester {
    pub fn on_connect(&mut self, incoming: bool, metadata: EventMetadata) {
        let ConnectionInfo { addr, pid, fd } = &metadata.id;
        if incoming {
            log::info!("{pid} accept {addr} {fd}");
        } else {
            log::info!("{pid} connect {addr} {fd}");
        }
    }

    pub fn on_disconnect(&mut self, metadata: EventMetadata) {
        let ConnectionInfo { addr, pid, fd } = &metadata.id;
        log::info!("{pid} disconnect {addr} {fd}");
    }

    pub fn on_data(&mut self, incoming: bool, metadata: EventMetadata, bytes: Vec<u8>) {
        let _ = (incoming, metadata);
        if bytes == b"test-is-passed" {
            println!("test is passed");
            std::process::exit(0);
        } else {
            assert!(bytes.iter().all(|v| *v == 0x11));
        }
    }
}
