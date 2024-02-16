use std::{
    fmt, io, mem,
    os::unix::io::AsRawFd,
    ptr, slice,
    sync::atomic::{AtomicBool, Ordering},
};

pub trait RingBufferData
where
    Self: Sized,
{
    type Error: fmt::Debug;

    fn from_rb_slice(slice: &[u8]) -> Result<Option<Self>, Self::Error>;
}

pub enum Output<D> {
    Value(D),
    /// How many bytes are remaining in the ring buffer
    Remaining(usize),
}

pub struct RingBuffer {
    fd: i32,
    mask: usize,
    consumer_pos_value: usize,
    // pointers to shared memory
    observer: RingBufferObserver,
    previous_distance: usize,
}

impl AsRawFd for RingBuffer {
    fn as_raw_fd(&self) -> i32 {
        self.fd
    }
}

struct RingBufferObserver {
    page_size: usize,
    data: Box<[usize]>,
    consumer_pos: Box<usize>,
    producer_pos: Box<usize>,
    epfd: i32,
    event: [epoll::Event; 1],
}

impl RingBufferObserver {
    #[allow(clippy::len_without_is_empty)]
    fn len(&self) -> usize {
        self.data.len() * mem::size_of::<usize>()
    }
}

impl AsRef<[u8]> for RingBufferObserver {
    fn as_ref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.data.as_ptr() as *const u8, self.len()) }
    }
}

enum Error {
    Overflown,
    WouldBlock,
}

impl RingBuffer {
    pub fn new(fd: i32, max_length: usize) -> io::Result<Self> {
        debug_assert_eq!(max_length & (max_length - 1), 0);

        // The layout is:
        // [consumer's page] [producer's page]       [data]
        //    0x1000 rw         0x1000 ro       max_length * 2 ro

        // it is a constant, most likely 0x1000
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;

        // consumers page, currently contains only one integer value,
        // offset where consumer should read;
        // map it read/write
        let consumer_pos = unsafe {
            let p = libc::mmap(
                ptr::null_mut(),
                page_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            if p == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }

            Box::from_raw(p as *mut usize)
        };

        // producers page and the buffer itself,
        // currently producers page contains only one integer value,
        // offset where producer has wrote, or still writing;
        // let's refer buffer data as a slice of `AtomicUsize` array
        // because we care only about data headers which sized and aligned by 8;
        // map it read only
        let (producer_pos, data) = unsafe {
            let p = libc::mmap(
                ptr::null_mut(),
                page_size + max_length * 2,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                page_size as i64,
            );
            if p == libc::MAP_FAILED {
                libc::munmap(Box::into_raw(consumer_pos) as *mut _, page_size);
                return Err(io::Error::last_os_error());
            }

            let length = max_length * 2 / mem::size_of::<usize>();
            let q = (p as usize) + page_size;
            let q = slice::from_raw_parts_mut(q as *mut usize, length);
            (
                Box::from_raw(p as *mut usize),
                Box::from_raw(q as *mut [usize]),
            )
        };

        log::info!(
            "new RingBuffer: fd: {}, page_size: 0x{:016x}, mask: 0x{:016x}",
            fd,
            page_size,
            max_length - 1
        );
        let event = epoll::Event::new(epoll::Events::EPOLLIN, 1);
        Ok(RingBuffer {
            fd,
            mask: max_length - 1,
            consumer_pos_value: 0,
            observer: RingBufferObserver {
                page_size,
                data,
                consumer_pos,
                producer_pos,
                epfd: {
                    let epfd = epoll::create(true)?;
                    epoll::ctl(epfd, epoll::ControlOptions::EPOLL_CTL_ADD, fd, event)?;
                    let epoll::Event { events, data } = event;
                    assert_eq!(events, epoll::Events::EPOLLIN.bits());
                    assert_eq!(data, 1);
                    epfd
                },
                event: [event],
            },
            previous_distance: 0,
        })
    }

    fn read_value<D>(&mut self) -> Result<(Option<D>, usize), Error>
    where
        D: RingBufferData,
    {
        let (v, remaining) = self.read_slice()?;
        if v.is_some() {
            self.read_finish();
        }
        Ok((v, remaining))
    }

    fn read_finish(&mut self) {
        *self.observer.consumer_pos = self.consumer_pos_value;
    }

    fn read_slice<D>(&mut self) -> Result<(Option<D>, usize), Error>
    where
        D: RingBufferData,
    {
        const HEADER_SIZE: usize = 8;
        const BUSY_BIT: usize = 1 << 31;
        const DISCARD_BIT: usize = 1 << 30;

        let pr_pos = *self.observer.producer_pos;
        if self.consumer_pos_value < pr_pos {
            // determine how far we are, how many unseen data is in the buffer
            let distance = pr_pos - self.consumer_pos_value;
            if distance > self.mask + 1 {
                return Err(Error::Overflown);
            }
            if distance > self.previous_distance {
                let percent = distance * 100 / (self.mask + 1);
                let previous_percent = self.previous_distance * 100 / (self.mask + 1);
                if percent > previous_percent && percent > 50 {
                    log::warn!("buffer is {percent}% full");
                }
            }
            self.previous_distance = distance;
            // the first 8 bytes of the memory slice is a header (length and flags)
            let (header, data_offset) = {
                let masked_pos = self.consumer_pos_value & self.mask;
                let index_in_array = masked_pos / mem::size_of::<usize>();
                let header = self.observer.data[index_in_array];
                // keep only 32 bits
                (header & 0xffffffff, masked_pos + HEADER_SIZE)
            };

            if header & BUSY_BIT != 0 {
                // nothing to read, kernel is writing to this slice right now
                return Err(Error::WouldBlock);
            }

            let (length, discard) = (header & !DISCARD_BIT, (header & DISCARD_BIT) != 0);

            if !discard {
                let c_pos = self.consumer_pos_value;
                log::debug!("SLICE: {c_pos:010x}, {pr_pos:010x}, length: 8 + {length:010x}");

                // check length, valid lengths are `header_size`
                // or `(header_size + (2 ^ n))`, where 8 <= n < 28
                // let header_size = 0x28;
                // if !(8..28).map(|n| 1 << n).any(|l| l + header_size == length) && (header_size != length) {
                //     log::error!("invalid length {length}");
                //     return Err(Error::WouldBlock);
                // }
            }

            // align the length by 8, and advance our position
            self.consumer_pos_value += HEADER_SIZE + (length + 7) / 8 * 8;
            let distance = pr_pos - self.consumer_pos_value;

            if !discard {
                // if not discard, yield the slice
                let s = unsafe {
                    slice::from_raw_parts(
                        ((self.observer.data.as_ptr() as usize) + data_offset) as *mut u8,
                        length,
                    )
                };
                match D::from_rb_slice(s) {
                    Err(err) => {
                        log::error!("rb parse data: {:?}", err);
                        Ok((None, distance))
                    }
                    Ok(None) => Ok((None, distance)),
                    Ok(Some(value)) => Ok((Some(value), distance)),
                }
            } else {
                Ok((None, distance))
            }
        } else {
            Err(Error::WouldBlock)
        }
    }

    fn wait_epoll(&mut self, terminating: &AtomicBool) {
        while !terminating.load(Ordering::SeqCst) {
            self.observer.event[0].events = 0;
            match epoll::wait(self.observer.epfd, 50, &mut self.observer.event) {
                Ok(0) => log::debug!("ringbuf wait timeout"),
                Ok(1) => {
                    let e = self.observer.event[0].events;
                    if e & epoll::Events::EPOLLIN.bits() != 0 {
                        break;
                    } else {
                        log::warn!("unexpected event {e}");
                    }
                }
                // poll should not return bigger then number of fds, we have 1
                Ok(r) => log::error!("ringbuf poll {}", r),
                Err(error) => {
                    if io::ErrorKind::Interrupted != error.kind() {
                        log::error!("ringbuf error: {:?}", error);
                    } else {
                        log::error!("inerrupted: {error:?}");
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    fn wait(&self, terminating: &AtomicBool) {
        let mut fds = libc::pollfd {
            fd: self.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        while !terminating.load(Ordering::SeqCst) {
            match unsafe { libc::poll(&mut fds, 1, 1_000) } {
                0 => log::debug!("ringbuf wait timeout"),
                1 => {
                    if fds.revents & libc::POLLIN != 0 {
                        break;
                    }
                }
                i32::MIN..=-1 => {
                    let error = io::Error::last_os_error();
                    if io::ErrorKind::Interrupted != error.kind() {
                        log::error!("ringbuf error: {:?}", error);
                    } else {
                        log::error!("inerrupted: {error:?}");
                    }
                }
                // poll should not return bigger then number of fds, we have 1
                r @ 2..=i32::MAX => log::error!("ringbuf poll {}", r),
            }
            fds.revents = 0;
        }
    }

    pub fn read_blocking<D>(&mut self, terminating: &AtomicBool) -> io::Result<(Option<D>, usize)>
    where
        D: RingBufferData,
    {
        let mut tries = 0;
        loop {
            if tries > 10 {
                log::debug!("cannot read ring buffer: {} attempts", tries);
            }
            match self.read_value() {
                Err(Error::WouldBlock) => {
                    self.wait_epoll(terminating);
                    if terminating.load(Ordering::SeqCst) {
                        break Err(io::Error::new(io::ErrorKind::Other, "terminate"));
                    }
                }
                Err(Error::Overflown) => {
                    return Err(io::Error::new(io::ErrorKind::Other, "overflow"));
                }
                Ok(value) => return Ok(value),
            }
            tries += 1;
        }
    }
}

impl Drop for RingBufferObserver {
    fn drop(&mut self) {
        epoll::close(self.epfd).unwrap_or_default();
        let len = self.len();
        let p = mem::replace(&mut self.consumer_pos, Box::new(0));
        let q = mem::replace(&mut self.producer_pos, Box::new(0));
        let data = mem::replace(&mut self.data, Box::new([]));
        unsafe {
            libc::munmap(Box::into_raw(p) as *mut _, self.page_size);
            libc::munmap(Box::into_raw(q) as *mut _, self.page_size + len);
        }
        Box::leak(data);
    }
}
