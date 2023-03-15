use core::time::Duration;
use std::{
    time::SystemTime,
    io::{self, BufRead, Read},
    fs::File,
};

/// Check whether the first command line argument matches the pattern
pub fn cmd_prefix_matches(pid: u32, pattern: &str) -> io::Result<bool> {
    let mut tries = 5;
    while tries > 0 {
        tries -= 1;

        let mut f = File::open(format!("/proc/{pid}/cmdline"))?;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        if s.starts_with(pattern) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Default)]
pub struct S {
    pub b_time: Option<SystemTime>,
}

impl S {
    /// Read the UNIX timestamp of when this device was booted.
    /// All eBPF timestamps refer to this time.
    pub fn read() -> io::Result<Self> {
        let mut s = S::default();
        for line in io::BufReader::new(File::open("/proc/stat")?).lines() {
            let line = line?;
            let mut it = line.split(' ');
            match it.next() {
                Some("btime") => {
                    s.b_time = it
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .and_then(|secs| {
                            SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs))
                        });
                }
                _ => (),
            }
        }

        Ok(s)
    }
}
