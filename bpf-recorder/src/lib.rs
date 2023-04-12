#![cfg_attr(feature = "kern", no_std)]

#[cfg(feature = "user")]
pub mod proc;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Event {
    pub fd: u32,
    pub pid: u32,
    pub tid: u32,
    pub ts0: u64,
    pub ts1: u64,
    pub tag: DataTag,
    pub size: i32,
}

impl Event {
    pub fn new(pid: u32, tid: u32, ts0: u64, ts1: u64) -> Self {
        Event {
            fd: 0,
            pid,
            tid,
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
    Alias,
    Random,
    GetSockOpt,
    SnarkWorker,
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
            DataTag::Alias,
            DataTag::Random,
            DataTag::GetSockOpt,
            DataTag::SnarkWorker,
        ];
        values.into_iter().find(|&v| v as u32 == c)
    }
}

#[cfg(feature = "user")]
pub mod sniffer_event {
    use std::net::{IpAddr, SocketAddr};

    use bpf_ring_buffer::RingBufferData;

    use super::{DataTag, Event};

    #[derive(Debug)]
    pub struct SnifferEvent {
        pub pid: u32,
        pub tid: u32,
        pub fd: u32,
        pub ts0: u64,
        pub ts1: u64,
        pub variant: SnifferEventVariant,
    }

    #[derive(Debug)]
    pub enum SnifferEventVariant {
        NewApp(String),
        NewSnarkWorkerApp,
        Bind(SocketAddr),
        IncomingConnection(SocketAddr),
        OutgoingConnection(SocketAddr),
        Disconnected,
        IncomingData(Vec<u8>),
        OutgoingData(Vec<u8>),
        Random(Vec<u8>),
        GetSockOpt(Vec<u8>),
        Error(DataTag, i32),
    }

    #[derive(Debug)]
    pub struct ErrorSliceTooShort;

    impl RingBufferData for SnifferEvent {
        type Error = ErrorSliceTooShort;

        fn from_rb_slice(slice: &[u8]) -> Result<Option<Self>, Self::Error> {
            use core::{mem, ptr};

            if slice.is_empty() {
                return Ok(None);
            }
            if slice.len() < mem::size_of::<Event>() {
                log::error!("slice too short: {}", hex::encode(slice));
                return Ok(None);
            }
            let event = unsafe { ptr::read::<Event>(slice.as_ptr() as *const _) };
            let Event {
                fd,
                pid,
                tid,
                ts0,
                ts1,
                tag,
                size,
            } = event;
            let ret = |variant| -> Result<Option<Self>, ErrorSliceTooShort> {
                Ok(Some(SnifferEvent {
                    pid,
                    tid,
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
            if slice.len() < mem::size_of::<Event>() + size {
                log::error!(
                    "expected {} bytes header + {size} bytes body, got {}, cannot recover",
                    mem::size_of::<Event>(),
                    slice.len(),
                );
                std::process::exit(1);
            }
            let data = &slice[mem::size_of::<Event>()..(mem::size_of::<Event>() + size)];
            if let DataTag::Accept | DataTag::Connect | DataTag::Bind = tag {
                let address_family = u16::from_ne_bytes(data[0..2].try_into().unwrap());
                let port = u16::from_be_bytes(data[2..4].try_into().unwrap());
                let addr = match address_family {
                    2 => {
                        let ip = <[u8; 4]>::try_from(&data[4..8]).unwrap();
                        SocketAddr::new(IpAddr::V4(ip.into()), port)
                    }
                    10 => {
                        let ip = <[u8; 16]>::try_from(&data[8..24]).unwrap();
                        SocketAddr::new(IpAddr::V6(ip.into()), port)
                    }
                    _ => return Ok(None),
                };
                match tag {
                    DataTag::Accept => ret(SnifferEventVariant::IncomingConnection(addr)),
                    DataTag::Connect => ret(SnifferEventVariant::OutgoingConnection(addr)),
                    DataTag::Bind => ret(SnifferEventVariant::Bind(addr)),
                    _ => unreachable!(),
                }
            } else if let DataTag::Read = tag {
                ret(SnifferEventVariant::IncomingData(data.to_vec()))
            } else if let DataTag::Write = tag {
                ret(SnifferEventVariant::OutgoingData(data.to_vec()))
            } else if let DataTag::Close = tag {
                ret(SnifferEventVariant::Disconnected)
            } else if let DataTag::Alias = tag {
                ret(SnifferEventVariant::NewApp(
                    String::from_utf8(data[..(data.len() - 1)].to_vec())
                        .unwrap_or("invalid_uft8_alias".to_string()),
                ))
            } else if let DataTag::Random = tag {
                ret(SnifferEventVariant::Random(data.to_vec()))
            } else if let DataTag::GetSockOpt = tag {
                ret(SnifferEventVariant::GetSockOpt(data.to_vec()))
            } else if let DataTag::SnarkWorker = tag {
                ret(SnifferEventVariant::NewSnarkWorkerApp)
            } else if let DataTag::Debug = tag {
                if data.len() == 4 {
                    log::info!("DEBUG: line: {}", u32::from_ne_bytes(data.try_into().unwrap()));
                } else {
                    log::info!("DEBUG: hex: {}", hex::encode(data));
                }
                Ok(None)
            } else {
                Ok(None)
            }
        }
    }
}
