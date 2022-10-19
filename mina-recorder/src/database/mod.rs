mod types;
pub use self::types::{StreamKind, StreamId, ConnectionId, ConnectionStats};

mod rocksdb;
pub use self::rocksdb::{DbFacade, DbGroup, DbStream, DbStrace};

mod params;
pub use self::params::Params;

mod index;

mod sorted_intersect;

mod core;
pub use self::core::{DbError, DbCore, RandomnessDatabase};

pub type DbResult<T> = Result<T, DbError>;
