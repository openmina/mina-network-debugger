#[derive(Clone, Copy)]
#[repr(C)]
pub struct Parameters {
    pub data: Variant,
    pub ts: u64,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum Variant {
    Empty {
        ptr: u64,
        len: u64,
    },

    Bind {
        fd: u32,
        addr_ptr: u64,
        addr_len: u64,
    },
    Connect {
        fd: u32,
        addr_ptr: u64,
        addr_len: u64,
    },
    Accept {
        listen_on_fd: u32,
        addr_ptr: u64,
        addr_len: u64,
    },
    Write {
        fd: u32,
        data_ptr: u64,
        _pad: u64,
    },
    Read {
        fd: u32,
        data_ptr: u64,
        _pad: u64,
    },
    Send {
        fd: u32,
        data_ptr: u64,
        _pad: u64,
    },
    Recv {
        fd: u32,
        data_ptr: u64,
        _pad: u64,
    },
}

impl Variant {
    pub fn ptr(&self) -> *const u8 {
        match self {
            Variant::Empty { ptr, .. } => *ptr as *const u8,
            Variant::Bind { addr_ptr, .. } => *addr_ptr as *const u8,
            Variant::Connect { addr_ptr, .. } => *addr_ptr as *const u8,
            Variant::Accept { addr_ptr, .. } => *addr_ptr as *const u8,
            Variant::Write { data_ptr, .. } => *data_ptr as *const u8,
            Variant::Read { data_ptr, .. } => *data_ptr as *const u8,
            Variant::Send { data_ptr, .. } => *data_ptr as *const u8,
            Variant::Recv { data_ptr, .. } => *data_ptr as *const u8,
        }
    }
}
