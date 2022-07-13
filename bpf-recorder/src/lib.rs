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

#[cfg(feature = "user")]
pub mod sniffer_event {
    use std::{net::SocketAddr, fmt};

    use bpf_ring_buffer::RingBufferData;

    use super::{Event, DataTag};

    #[derive(Debug)]
    pub struct SnifferEvent {
        pub pid: u32,
        pub fd: u32,
        pub ts0: u64,
        pub ts1: u64,
        pub variant: SnifferEventVariant,
    }

    #[derive(Debug)]
    pub enum SnifferEventVariant {
        IncomingConnection(SocketAddr),
        OutgoingConnection(SocketAddr),
        Disconnected,
        IncomingData(Vec<u8>),
        OutgoingData(Vec<u8>),
        Error(DataTag, i32),
    }

    impl fmt::Display for SnifferEventVariant {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                SnifferEventVariant::IncomingConnection(addr) => write!(f, "<-> accept {addr}"),
                SnifferEventVariant::OutgoingConnection(addr) => write!(f, "<-> connect {addr}"),
                SnifferEventVariant::Disconnected => write!(f, "<-x->"),
                SnifferEventVariant::IncomingData(data) => write!(f, "<- {}", hex::encode(data)),
                SnifferEventVariant::OutgoingData(data) => write!(f, "-> {}", hex::encode(data)),
                SnifferEventVariant::Error(tag, code) => write!(f, "tag: {tag:?}, code: {code}"),
            }
        }
    }

    impl fmt::Display for SnifferEvent {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let SnifferEvent { pid, fd, ts0, ts1, .. } = self;
            write!(f, "{pid}:{fd}, {ts0} - {ts1}")
        }
    }

    #[derive(Debug)]
    pub struct ErrorSliceTooShort;

    impl RingBufferData for SnifferEvent {
        type Error = ErrorSliceTooShort;

        fn from_rb_slice(slice: &[u8]) -> Result<Option<Self>, Self::Error> {
            if slice.len() < 32 {
                return Err(ErrorSliceTooShort);
            }
            let event = Event::from_bytes(&slice[..32]);
            let Event { fd, pid, ts0, ts1, tag, size } = event;
            let ret = |variant| -> Result<Option<Self>, ErrorSliceTooShort> {
                Ok(Some(SnifferEvent {
                    pid,
                    fd,
                    ts0,
                    ts1,
                    variant,
                }))
            };
            if size < 0 {
                return ret(SnifferEventVariant::Error(tag, size));
            }
            let size = size as usize;
            if slice.len() < 32 + size {
                return Err(ErrorSliceTooShort);
            }
            let data = &slice[32..(32 + size)];
            if let DataTag::Accept | DataTag::Connect = tag {
                let ty = u16::from_ne_bytes(data[..2].try_into().unwrap());
                let port = u16::from_be_bytes(data[2..4].try_into().unwrap());
                if (ty != 2 && ty != 10) || port == 0 || port == 53 || port == 80 || port == 443 {
                    // need only ipv4 (ty == 2) or ipv6 (ty == 10)
                    // also, ignore some ports
                    Ok(None)
                } else {
                    // TODO: real address
                    let addr = SocketAddr::new("127.0.0.1".parse().unwrap(), port);
                    match tag {
                        DataTag::Accept => ret(SnifferEventVariant::IncomingConnection(addr)),
                        _ => ret(SnifferEventVariant::OutgoingConnection(addr)),
                    }
                }
            } else if let DataTag::Read = tag {
                ret(SnifferEventVariant::IncomingData(data.to_vec()))
            } else if let DataTag::Write = tag {
                ret(SnifferEventVariant::OutgoingData(data.to_vec()))
            } else if let DataTag::Close = tag {
                ret(SnifferEventVariant::Disconnected)
            } else {
                Ok(None)
            }
        }
    }
}
