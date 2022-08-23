use std::{thread, path::Path};

use warp::{
    Filter, Rejection, Reply,
    reply::{WithStatus, Json, self},
    http::StatusCode,
};

use super::database::{DbCore, DbFacade, Params};

fn connection(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("connection" / u64).map(move |id: u64| -> reply::WithStatus<Json> {
        match db.fetch_connection(id) {
            Ok(v) => reply::with_status(reply::json(&v), StatusCode::OK),
            Err(err) => reply::with_status(
                reply::json(&err.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
    })
}

fn messages(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("messages").and(warp::query::query()).map(
        move |params: Params| -> WithStatus<Json> {
            match params.validate() {
                Ok(valid) => {
                    let v = db.fetch_messages(&valid);
                    reply::with_status(reply::json(&v.collect::<Vec<_>>()), StatusCode::OK)
                }
                Err(err) => reply::with_status(
                    reply::json(&err.to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR,
                ),
            }
        },
    )
}

fn message(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("message" / u64).map(move |id: u64| -> reply::WithStatus<Json> {
        match db.fetch_full_message(id) {
            Ok(v) => reply::with_status(reply::json(&v), StatusCode::OK),
            Err(err) => reply::with_status(
                reply::json(&err.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
    })
}

fn message_hex(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("message_hex" / u64).map(move |id: u64| -> reply::WithStatus<Json> {
        match db.fetch_full_message_hex(id) {
            Ok(v) => reply::with_status(reply::json(&v), StatusCode::OK),
            Err(err) => reply::with_status(
                reply::json(&err.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
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
            let d = serde_json::from_str::<serde_json::Value>(s)
                .expect("static file \"openapi.json\" must be valid json");
            reply::with_status(reply::json(&d), StatusCode::OK)
        })
}

fn routes(
    db: DbCore,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone + Sync + Send + 'static {
    use warp::reply::with;

    warp::get()
        .and(
            connection(db.clone())
                .or(message(db.clone()))
                .or(message_hex(db.clone()))
                .or(messages(db))
                .or(version().or(openapi())),
        )
        .with(with::header("Content-Type", "application/json"))
        .with(with::header("Access-Control-Allow-Origin", "*"))
}

pub fn spawn<P>(port: u16, path: P) -> (DbFacade, impl FnOnce(), thread::JoinHandle<()>)
where
    P: AsRef<Path>,
{
    use std::process;
    use tokio::{sync::oneshot, runtime::Runtime};

    let rt = match Runtime::new() {
        Ok(v) => v,
        Err(err) => {
            log::error!("fatal: {err}");
            process::exit(1);
        }
    };
    let _guard = rt.enter();
    let (tx, rx) = oneshot::channel();

    let db = match DbFacade::open(path) {
        Ok(v) => v,
        Err(err) => {
            log::error!("fatal: {err}");
            process::exit(1);
        }
    };
    let addr = ([0, 0, 0, 0], port);
    let (_, server) =
        warp::serve(routes(db.core())).bind_with_graceful_shutdown(addr, async move {
            rx.await.expect("corresponding sender should exist");
            log::info!("terminating http server...");
        });
    let handle = thread::spawn(move || rt.block_on(server));
    let callback = move || tx.send(()).expect("corresponding receiver should exist");
    (db, callback, handle)
}
