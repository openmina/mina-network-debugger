use std::{
    net::{TcpListener, TcpStream},
    io::{Read, Write},
    sync::Arc,
    thread, fs,
    time::Duration,
};

const P2P_PORT: u16 = 8302;

const WIDTH: usize = 16;
const SERIES: usize = 16;

fn main() {
    let listener_thread = thread::spawn(move || {
        let listener = TcpListener::bind(("127.0.0.1", P2P_PORT)).unwrap();

        let mut stream_threads = vec![];
        for _ in 0..((WIDTH * SERIES) + 1) {
            let (mut stream, _) = listener.accept().unwrap();
            let stream_thread = thread::spawn(move || {
                let mut buf = [0; 0x1000];
                while stream.read(&mut buf).unwrap() != 0 {}    
            });
            stream_threads.push(stream_thread);
        }

        for stream_thread in stream_threads {
            stream_thread.join().unwrap();
        }
    });

    let data = Arc::new([0x11; 0x10000]);
    let data_false = Arc::new([0x22; 0x10000]);
    for _ in 0..SERIES {
        let mut stream_threads = vec![];
        for _ in 0..WIDTH {
            let stream_thread = {
                let data = data.clone();
                let data_false = data_false.clone();
                thread::spawn(move || {
                    let mut stream = TcpStream::connect(("127.0.0.1", P2P_PORT)).unwrap();
                    for _ in 0..16 {
                        stream
                            .write_all(&data[..(0x1000 + (rand::random::<usize>() % 0xf000))])
                            .unwrap();
                    }
                    fs::File::create("/tmp/test")
                        .unwrap()
                        .write_all(&data_false[..0x1000])
                        .unwrap();
                })
            };
            stream_threads.push(stream_thread);
        }
        thread::sleep(Duration::from_millis(200));

        for stream_thread in stream_threads {
            stream_thread.join().unwrap();
        }
    }

    let mut stream = TcpStream::connect(("127.0.0.1", P2P_PORT)).unwrap();
    stream
        .write_all(b"test-is-passed")
        .unwrap();
    drop(stream);

    listener_thread.join().unwrap();
}
