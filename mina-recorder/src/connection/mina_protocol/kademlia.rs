use serde::Serialize;

#[allow(clippy::derive_partial_eq_without_eq)]
mod pb {
    include!(concat!(env!("OUT_DIR"), "/kad.pb.rs"));
}

pub fn parse(bytes: Vec<u8>) -> impl Serialize {
    #[derive(Serialize)]
    pub enum MessageType {
        PutValue = 0,
        GetValue = 1,
        AddProvider = 2,
        GetProviders = 3,
        FindNode = 4,
        Ping = 5,
    }

    #[derive(Serialize)]
    pub struct Record {
        key: String,
        value: String,
        /// Time the record was received, set by receiver
        /// Formatted according to <https://datatracker.ietf.org/doc/html/rfc3339>
        time_received: String,
    }

    impl From<pb::Record> for Record {
        fn from(v: pb::Record) -> Self {
            Record {
                key: hex::encode(v.key),
                value: hex::encode(v.value),
                time_received: v.time_received,
            }
        }
    }

    #[derive(Serialize)]
    pub enum ConnectionType {
        /// sender does not have a connection to peer, and no extra information (default)
        NotConnected = 0,
        /// sender has a live connection to peer
        Connected = 1,
        /// sender recently connected to peer
        CanConnect = 2,
        /// sender recently tried to connect to peer repeatedly but failed to connect
        /// ("try" here is loose, but this should signal "made strong effort, failed")
        CannotConnect = 3,
    }

    #[derive(Serialize)]
    pub struct Peer {
        id: String,
        addrs: Vec<String>,
        connection: ConnectionType,
    }

    impl From<pb::message::Peer> for Peer {
        fn from(v: pb::message::Peer) -> Self {
            Peer {
                id: hex::encode(&v.id),
                addrs: v
                    .addrs
                    .iter()
                    .map(|addr| {
                        let mut acc = String::new();
                        let mut input = addr.as_slice();
                        while !input.is_empty() {
                            match multiaddr::Protocol::from_bytes(input) {
                                Ok((p, i)) => {
                                    input = i;
                                    acc = format!("{acc}{p}");
                                }
                                Err(err) => {
                                    input = &[];
                                    acc = format!("{acc}{err}");
                                }
                            }
                        }
                        acc
                    })
                    .collect(),
                connection: match v.connection() {
                    pb::message::ConnectionType::NotConnected => ConnectionType::NotConnected,
                    pb::message::ConnectionType::Connected => ConnectionType::Connected,
                    pb::message::ConnectionType::CanConnect => ConnectionType::CanConnect,
                    pb::message::ConnectionType::CannotConnect => ConnectionType::CannotConnect,
                },
            }
        }
    }

    #[derive(Serialize)]
    pub struct T {
        ty: MessageType,
        // TODO: parse further
        key: String,
        record: Option<Record>,
        closer_peers: Vec<Peer>,
        provider_peers: Vec<Peer>,
    }

    use prost::{bytes::Bytes, Message};

    let buf = Bytes::from(bytes);
    let msg = <pb::Message as Message>::decode_length_delimited(buf).unwrap();

    T {
        ty: match msg.r#type() {
            pb::message::MessageType::PutValue => MessageType::PutValue,
            pb::message::MessageType::GetValue => MessageType::GetValue,
            pb::message::MessageType::AddProvider => MessageType::AddProvider,
            pb::message::MessageType::GetProviders => MessageType::GetProviders,
            pb::message::MessageType::FindNode => MessageType::FindNode,
            pb::message::MessageType::Ping => MessageType::Ping,
        },
        key: hex::encode(msg.key),
        record: msg.record.map(From::from),
        closer_peers: msg.closer_peers.into_iter().map(From::from).collect(),
        provider_peers: msg.provider_peers.into_iter().map(From::from).collect(),
    }
}
