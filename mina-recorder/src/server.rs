use std::thread;

use warp::{
    Filter, Rejection, Reply,
    reply::{WithStatus, Json, self},
    http::StatusCode,
};

use super::database::{DbCore, DbFacade};

fn connections(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("connections")
        .and(warp::query::query())
        .map(move |params| -> WithStatus<Json> {
            let v = db.fetch_connections(&params);
            reply::with_status(reply::json(&v.collect::<Vec<_>>()), StatusCode::OK)
        })
}

fn messages(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("messages")
        .and(warp::query::query())
        .map(move |params| -> WithStatus<Json> {
            let v = db.fetch_messages(&params);
            reply::with_status(reply::json(&v.collect::<Vec<_>>()), StatusCode::OK)
        })
}

fn version(
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("version")
        .and(warp::query::query())
        .map(move |()| -> reply::WithStatus<Json> {
            reply::with_status(reply::json(&env!("GIT_HASH")), StatusCode::OK)
        })
}

fn openapi(
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("openapi")
        .and(warp::query::query())
        .map(move |()| -> reply::WithStatus<Json> {
            let s = include_str!("openapi.json");
            let d = serde_json::from_str::<serde_json::Value>(s).unwrap();
            reply::with_status(reply::json(&d), StatusCode::OK)
        })
}

fn routes(
    db: DbCore,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone + Sync + Send + 'static {
    use warp::reply::with;

    warp::get()
        .and(
            connections(db.clone())
                .or(messages(db))
                .or(version().or(openapi())),
        )
        .with(with::header("Content-Type", "application/json"))
        .with(with::header("Access-Control-Allow-Origin", "*"))
}

pub fn run() -> (DbFacade, impl FnOnce(), thread::JoinHandle<()>) {
    use tokio::{sync::oneshot, runtime::Runtime};

    let rt = Runtime::new().unwrap();
    let _guard = rt.enter();
    let (tx, rx) = oneshot::channel();

    let db = DbFacade::open("target/db").unwrap();
    let addr = ([0, 0, 0, 0], 8000u16);
    let (_, server) =
        warp::serve(routes(db.core())).bind_with_graceful_shutdown(addr, async move {
            rx.await.unwrap();
            log::info!("terminating http server...");
        });
    let handle = thread::spawn(move || rt.block_on(server));
    (db, move || tx.send(()).unwrap(), handle)
}
