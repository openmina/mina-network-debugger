#[derive(Default)]
pub struct SnarkWorkerState {

}

impl SnarkWorkerState {
    pub fn handle_data(&mut self, incoming: bool, fd: u32, data: Vec<u8>) {
        let arrow = if incoming {
            "<-"
        } else {
            "->"
        };
        log::info!("snark worker {fd} {arrow} {}", hex::encode(data));
    }
}

#[cfg(test)]
#[test]
fn snark_worked_rpc() {
    let data_str = "250000000000000002020021003d0e4c640f07d941490e4c640f07d941000000000000f0bf5e0e4c640f07d941";
    let data = hex::decode(data_str).unwrap();
    let msg = crate::decode::rpc::parse(data, false).unwrap();
    println!("{msg}");
}
