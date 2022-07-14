use std::{
    collections::BTreeMap,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use salsa20::cipher::generic_array::{typenum, GenericArray};
use salsa20::{
    cipher::{KeyIvInit as _, StreamCipher},
    XSalsa20,
};

use super::connection::{self, ConnectionId, Event, Link};

#[derive(Default)]
pub struct P2pRecorder {
    cns: BTreeMap<ConnectionId, Task>,
}

struct Task {
    cipher_in: Option<XSalsa20>,
    cipher_out: Option<XSalsa20>,
    link: Link,
    future: Pin<Box<dyn Future<Output = ()>>>,
}

impl Task {
    pub fn shared_secret() -> GenericArray<u8, typenum::U32> {
        use blake2::{
            digest::{Update, VariableOutput},
            Blake2bVar,
        };

        // TODO: seed
        let seed = b"/coda/0.0.1/dd0f3f26be5a093f00077d1cd5d89abc253c95f301e9c12ae59e2d7c6052cc4d";
        let mut key = GenericArray::default();
        Blake2bVar::new(32)
            .unwrap()
            .chain(seed)
            .finalize_variable(&mut key)
            .unwrap();
        key
    }
}

impl P2pRecorder {
    pub fn on_connect(&mut self, incoming: bool, alias: String, addr: SocketAddr, fd: u32) {
        if incoming {
            log::info!("{alias} accept {addr} {fd}");
        } else {
            log::info!("{alias} connect {addr} {fd}");
        }
        let id = ConnectionId { alias, addr, fd };
        let link = Link::default();
        let task = Task {
            cipher_in: None,
            cipher_out: None,
            link: link.clone(),
            future: Box::pin(connection::run(id.clone(), link)),
        };
        self.cns.insert(id, task);
    }

    pub fn on_disconnect(&mut self, alias: String, addr: SocketAddr, fd: u32) {
        log::info!("{alias} disconnect {addr} {fd}");
        let id = ConnectionId { alias, addr, fd };
        self.cns.remove(&id);
    }

    pub fn on_data(
        &mut self,
        incoming: bool,
        alias: String,
        addr: SocketAddr,
        fd: u32,
        mut bytes: Vec<u8>,
    ) {
        let id = ConnectionId { alias, addr, fd };
        if let Some(cn) = self.cns.get_mut(&id) {
            let cipher = if incoming {
                &mut cn.cipher_in
            } else {
                &mut cn.cipher_out
            };
            if let Some(cipher) = cipher {
                cipher.apply_keystream(&mut bytes);
                let waker = noop_waker::noop_waker();
                let mut cx = Context::from_waker(&waker);
                cn.link.put_event(Event::Data { incoming, bytes });
                match Future::poll(cn.future.as_mut(), &mut cx) {
                    Poll::Pending => (),
                    Poll::Ready(()) => drop(self.cns.remove(&id)),
                }
            } else {
                assert_eq!(bytes.len(), 24);
                let key = Task::shared_secret();
                *cipher = Some(XSalsa20::new(&key, GenericArray::from_slice(&bytes)));
            }
        }
    }
}
