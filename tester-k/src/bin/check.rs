use std::net::IpAddr;

fn main() {
    use std::{thread, time::Duration};

    use tester_k::tcpflow::TcpFlow;

    let t = TcpFlow::run(IpAddr::from([0, 0, 0, 0])).unwrap();
    thread::sleep(Duration::from_secs(10));
    let report = t.stop();
    println!("{report:?}");
}
