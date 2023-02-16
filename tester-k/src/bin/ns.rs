use std::time::Duration;

fn main() {
    use tester_k::netstat::Netstat;

    env_logger::init();

    let ns = Netstat::run().unwrap();
    std::thread::sleep(Duration::from_secs(10));
    for e in ns.stop() {
        println!("{} {}", e.local, e.remote);
    }
}
