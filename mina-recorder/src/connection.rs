use std::{
    cell::RefCell,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId {
    pub alias: String,
    pub addr: SocketAddr,
    pub fd: u32,
}

#[derive(Default, Clone)]
pub struct Link(Rc<RefCell<Option<Event>>>);

pub enum Event {
    Data { incoming: bool, bytes: Vec<u8> },
}

impl Link {
    pub fn put_event(&self, event: Event) {
        *self.0.borrow_mut() = Some(event);
    }

    pub async fn next_event(&self) -> Event {
        struct Throw<'a>(&'a Link);

        impl<'a> Future for Throw<'a> {
            type Output = Event;

            fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                match self.as_mut().0 .0.borrow_mut().take() {
                    None => Poll::Pending,
                    Some(a) => Poll::Ready(a),
                }
            }
        }

        Throw(self).await
    }
}

pub async fn run(id: ConnectionId, link: Link) {
    let ConnectionId { alias, addr, fd } = id;

    loop {
        let Event::Data { incoming, bytes } = link.next_event().await;
        if incoming {
            log::info!(
                "{addr} {fd} -> {alias} {}, {}",
                bytes.len(),
                hex::encode(&bytes)
            );
        } else {
            log::info!(
                "{addr} {fd} <- {alias} {}, {}",
                bytes.len(),
                hex::encode(&bytes)
            );
        }
    }
}
