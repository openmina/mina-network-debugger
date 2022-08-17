mod types;
pub use self::types::{StreamKind, StreamId};

mod rocksdb;
pub use self::rocksdb::{DbFacade, DbGroup, DbStream};

mod index;

mod core;
pub use self::core::{DbError, DbCore};

pub type DbResult<T> = Result<T, DbError>;
