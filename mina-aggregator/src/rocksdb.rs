use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use radiation::{Collection, Emit, AbsorbExt, nom, ParseError};
use thiserror::Error;

use super::database::GlobalBlockState;

pub struct DbInner(rocksdb::DB);

#[derive(Debug, Error)]
pub enum DbError {
    #[error("{_0}")]
    Inner(#[from] rocksdb::Error),
    #[error("{_0}")]
    Parse(#[from] nom::Err<ParseError<Vec<u8>>>),
}

impl DbInner {
    const TTL: Duration = Duration::from_secs(0);

    pub fn open<P>(path: P) -> Result<Self, DbError>
    where
        P: AsRef<Path>,
    {
        let path = PathBuf::from(path.as_ref());

        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let cfs = [rocksdb::ColumnFamilyDescriptor::new(
            "block",
            Default::default(),
        )];

        let inner =
            rocksdb::DB::open_cf_descriptors_with_ttl(&opts, path.join("rocksdb"), cfs, Self::TTL)?;

        Ok(DbInner(inner))
    }

    fn block(&self) -> &rocksdb::ColumnFamily {
        self.0.cf_handle("block").expect("must exist")
    }

    pub fn put_block(
        &self,
        height: u32,
        value: impl IntoIterator<Item = GlobalBlockState> + Clone,
    ) -> Result<(), DbError> {
        let value = Collection(value);
        let bytes = value.chain(vec![]);
        self.0.put_cf(self.block(), height.to_be_bytes(), bytes)?;

        Ok(())
    }

    pub fn fetch_block(&self, height: u32) -> Result<Option<Vec<GlobalBlockState>>, DbError> {
        let b = match self.0.get_cf(self.block(), height.to_be_bytes())? {
            Some(v) => v,
            None => return Ok(None),
        };
        let Collection(v) = AbsorbExt::absorb_ext(&b).map_err(|e| e.map(ParseError::into_vec))?;
        Ok(Some(v))
    }
}
