use snow::{Builder, HandshakeState, TransportState};

use super::{ConnectionId, HandleData};

#[allow(dead_code)]
pub struct State<Inner> {
    initiator: St,
    responder: St,
    inner: Inner,
}

impl<Inner> Default for State<Inner>
where
    Inner: Default,
{
    fn default() -> Self {
        let i = Builder::new("Noise_XX_25519_ChaChaPoly_SHA256".parse().unwrap())
            .local_private_key(&[0; 32])
            .build_initiator()
            .unwrap();
        let r = Builder::new("Noise_XX_25519_ChaChaPoly_SHA256".parse().unwrap())
            .local_private_key(&[0; 32])
            .build_responder()
            .unwrap();
        State {
            initiator: St::Handshake(i),
            responder: St::Handshake(r),
            inner: Inner::default(),
        }
    }
}

#[allow(dead_code)]
enum St {
    Nothing,
    Handshake(HandshakeState),
    Transport(TransportState),
}

impl<Inner> HandleData for State<Inner> {
    fn on_data(&mut self, id: ConnectionId, incoming: bool, bytes: Vec<u8>) {
        // let st = if incoming {
        //     &mut self.responder
        // } else {
        //     &mut self.initiator
        // };
        // let mut payload = vec![0; bytes.len() - 16];
        // let len = match std::mem::replace(st, St::Nothing) {
        //     St::Nothing => panic!(),
        //     St::Handshake(mut handshake) => {
        //         let len = handshake.read_message(&bytes[2..], &mut payload).unwrap();
        //         if handshake.is_handshake_finished() {
        //             *st = St::Transport(handshake.into_transport_mode().unwrap());
        //         } else {
        //             *st = St::Handshake(handshake);
        //         }
        //         len
        //     },
        //     St::Transport(mut transport) => {
        //         let len = transport.read_message(&bytes[2..], &mut payload).unwrap();
        //         *st = St::Transport(transport);
        //         len
        //     },
        // };
        // let bytes = &payload[..len];

        let ConnectionId { alias, addr, fd } = id;
        let arrow = if incoming { "->" } else { "<-" };
        let len = bytes.len();
        log::info!("{addr} {fd} {arrow} {alias} {len} \"{}\"", hex::encode(bytes));

        let _ = &mut self.inner;
    }
}
