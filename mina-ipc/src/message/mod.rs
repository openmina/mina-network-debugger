mod config;
pub use self::config::{Config, GatingConfig};

/// daemon -> libp2p (incoming from libp2p perspective)
pub mod incoming;

/// daemon <- libp2p (outgoing from libp2p perspective)
pub mod outgoing;

mod consensus_message;
pub use self::consensus_message::ConsensusMessage;

mod checksum;
pub use self::checksum::{Checksum, ChecksumIo, ChecksumPair, ChecksumTotal};

mod buffer;
pub use self::buffer::Buffer;

use capnp::traits::FromPointerBuilder;

pub trait CapnpEncode<'a> {
    type Builder: FromPointerBuilder<'a>;

    fn build(&self, builder: Self::Builder) -> capnp::Result<()>;
}
