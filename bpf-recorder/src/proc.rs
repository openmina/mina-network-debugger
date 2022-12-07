use core::time::Duration;
use std::{time::SystemTime, io::{self, BufRead}, fs::File};

#[derive(Default)]
pub struct S {
    pub b_time: Option<SystemTime>,
}

impl S {
    pub fn read() -> io::Result<Self> {
        let mut s = S::default();
        for line in io::BufReader::new(File::open("/proc/stat")?).lines() {
            let line = line?;
            let mut it = line.split(' ');
            match it.next() {
                Some("btime") => {
                    s.b_time = it.next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .and_then(|secs| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs)));
                }
                _ => (),
            }
        }

        Ok(s)
    }
}
