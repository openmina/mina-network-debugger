#![cfg_attr(feature = "kern", no_std)]

#[derive(Clone, Copy, Debug)]
#[repr(packed)]
pub struct Event {
    pub fd: u32,
    pub pid: u32,
    pub ts0: u64,
    pub ts1: u64,
    pub tag: DataTag,
    pub size: i32,
}

impl Event {
    pub fn new(pid: u32, ts0: u64, ts1: u64) -> Self {
        Event {
            fd: 0,
            pid,
            ts0,
            ts1,
            tag: DataTag::Debug,
            size: 0,
        }
    }

    pub fn set_tag_fd(mut self, tag: DataTag, fd: u32) -> Self {
        self.fd = fd;
        self.tag = tag;
        self
    }

    pub fn set_ok(mut self, size: u64) -> Self {
        self.size = size as _;
        self
    }

    pub fn set_err(mut self, code: i64) -> Self {
        self.size = code as _;
        self
    }

    pub fn from_bytes(b: &[u8]) -> Self {
        assert_eq!(b.len(), 32);
        Event {
            fd: u32::from_ne_bytes(b[0..4].try_into().unwrap()),
            pid: u32::from_ne_bytes(b[4..8].try_into().unwrap()),
            ts0: u64::from_ne_bytes(b[8..16].try_into().unwrap()),
            ts1: u64::from_ne_bytes(b[16..24].try_into().unwrap()),
            tag: DataTag::from_u32(u32::from_ne_bytes(b[24..28].try_into().unwrap())).unwrap(),
            size: i32::from_ne_bytes(b[28..32].try_into().unwrap()),
        }
    }
}

#[allow(dead_code)]
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum DataTag {
    Debug,
    Close,
    Connect,
    Bind,
    Listen,
    Accept,
    Write,
    Read,
}

impl DataTag {
    pub fn from_u32(c: u32) -> Option<Self> {
        let values = [
            DataTag::Debug,
            DataTag::Close,
            DataTag::Connect,
            DataTag::Bind,
            DataTag::Listen,
            DataTag::Accept,
            DataTag::Write,
            DataTag::Read,
        ];
        for v in values {
            if v as u32 == c {
                return Some(v);
            }
        }
        None
    }
}
