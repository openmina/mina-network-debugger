mod types;
pub use self::types::{StreamKind, StreamMeta};

mod rocksdb;
pub use self::rocksdb::{DbError, DbFacade, DbGroup, DbStream};

mod reader;

mod server;

use std::thread;

pub fn run_server() -> (DbFacade, impl FnOnce(), thread::JoinHandle<()>) {
    use tokio::{sync::oneshot, runtime::Runtime};

    let rt = Runtime::new().unwrap();
    let _guard = rt.enter();
    let (tx, rx) = oneshot::channel();

    let db = DbFacade::open("target/db").unwrap();
    let addr = ([0, 0, 0, 0], 8000u16);
    let (_, server) =
        warp::serve(server::routes(db.core())).bind_with_graceful_shutdown(addr, async move {
            rx.await.unwrap();
            log::info!("terminating http server...");
        });
    let handle = thread::spawn(move || rt.block_on(server));
    (db, move || tx.send(()).unwrap(), handle)
}
