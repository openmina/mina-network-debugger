use std::marker::PhantomData;

use crate::database::DbStream;

use super::{HandleData, DirectedId, DynamicProtocol, Cx, Db, DbResult};

#[derive(Default)]
pub struct State<Inner> {
    stream: Option<DbStream>,
    inner: PhantomData<Inner>,
}

impl<Inner> DynamicProtocol for State<Inner> {
    fn from_name(name: &str, _: u64, _: bool) -> Self {
        assert_eq!(name, "/coda/yamux/1.0.0");
        State {
            stream: None,
            inner: PhantomData,
        }
    }
}

impl<Inner> HandleData for State<Inner>
where
    Inner: HandleData + From<(u64, bool)>,
{
    fn on_data(&mut self, id: DirectedId, bytes: &mut [u8], cx: &mut Cx, db: &Db) -> DbResult<()> {
        let _ = (id, bytes, cx, db, &mut self.stream);
        Ok(())
    }
}
