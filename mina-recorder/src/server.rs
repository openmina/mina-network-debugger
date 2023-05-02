use std::{thread, path::Path};

use warp::{
    Filter, Rejection, Reply,
    reply::{WithStatus, Json, self},
    http::StatusCode,
};

use crate::{meshsub_stats::BlockStat, application::Application};

use super::database::{DbCore, DbFacade, Params};

fn connection(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("connection" / u64).map(move |id: u64| -> reply::WithStatus<Json> {
        match db.fetch_connection(id) {
            Ok(v) => {
                let v = v.post_process(None);
                reply::with_status(reply::json(&v), StatusCode::OK)
            }
            Err(err) => reply::with_status(
                reply::json(&err.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
    })
}

fn connections(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("connections").and(warp::query::query()).map(
        move |params: Params| -> WithStatus<Json> {
            match params.validate_connection() {
                Ok(valid) => {
                    let v = db.fetch_connections(&valid);
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

fn message_bin(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Vec<u8>>,), Error = Rejection> + Clone + Sync + Send + 'static
{
    warp::path!("message_bin" / u64).map(move |id: u64| -> reply::WithStatus<Vec<u8>> {
        match db.fetch_full_message_bin(id) {
            Ok(v) => reply::with_status(v, StatusCode::OK),
            Err(err) => reply::with_status(
                err.to_string().as_bytes().to_vec(),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
    })
}

fn stats(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("block_v1" / u32).map(move |id| -> WithStatus<Json> {
        let v = db.fetch_stats(id).map(|(_, v)| v);
        reply::with_status(reply::json(&v), StatusCode::OK)
    })
}

fn stats_block_v2(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("block" / u32).map(move |height| -> WithStatus<Json> {
        let events = db.fetch_stats_block_v2(height);
        let v = BlockStat { height, events };
        reply::with_status(reply::json(&v), StatusCode::OK)
    })
}

fn stats_last(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("block_v1" / "last").map(move || -> WithStatus<Json> {
        let v = db.fetch_last_stat().map(|(_, v)| v);
        reply::with_status(reply::json(&v), StatusCode::OK)
    })
}

fn stats_latest(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("block_v1" / "latest").map(move || -> WithStatus<Json> {
        let v = db.fetch_last_stat().map(|(_, v)| v);
        reply::with_status(reply::json(&v), StatusCode::OK)
    })
}

fn stats_block_v2_latest(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("block" / "latest").map(move || -> WithStatus<Json> {
        let v = db
            .fetch_last_stat_block_v2()
            .map(|(height, events)| BlockStat { height, events });
        reply::with_status(reply::json(&v), StatusCode::OK)
    })
}

fn stats_tx(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("tx" / u32).map(move |id| -> WithStatus<Json> {
        let v = db.fetch_stats_tx(id);
        match v {
            Ok(v) => {
                let v = v.map(|(_, v)| v);
                reply::with_status(reply::json(&v), StatusCode::OK)
            }
            Err(err) => reply::with_status(
                reply::json(&err.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
    })
}

fn stats_tx_latest(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("tx" / "latest").map(move || -> WithStatus<Json> {
        let v = db.fetch_last_stat_tx().map(|(_, v)| v);
        reply::with_status(reply::json(&v), StatusCode::OK)
    })
}

fn snark(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("snark" / String).map(move |hash| -> WithStatus<Json> {
        match db.fetch_snark_by_hash(hash) {
            Ok(v) => reply::with_status(reply::json(&v), StatusCode::OK),
            Err(err) => reply::with_status(
                reply::json(&err.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
    })
}

#[derive(serde::Deserialize)]
pub struct BlockParams {
    all: Option<bool>,
}

impl BlockParams {
    // default is show all without filtering
    fn all(&self) -> bool {
        self.all.unwrap_or(true)
    }
}

fn capnp(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("capnp" / "block" / u32)
        .and(warp::query::query())
        .map(move |height, params: BlockParams| -> WithStatus<Json> {
            let v = db.fetch_capnp(height, params.all()).collect::<Vec<_>>();
            reply::with_status(reply::json(&v), StatusCode::OK)
        })
}

fn libp2p_ipc(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("libp2p_ipc" / "block" / u32)
        .and(warp::query::query())
        .map(move |height, params: BlockParams| -> WithStatus<Json> {
            let v = db.fetch_capnp(height, params.all()).collect::<Vec<_>>();
            reply::with_status(reply::json(&v), StatusCode::OK)
        })
}

fn libp2p_ipc_all(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("libp2p_ipc" / "block" / "all").map(move || -> WithStatus<Json> {
        let v = db.fetch_capnp_all().collect::<Vec<_>>();
        reply::with_status(reply::json(&v), StatusCode::OK)
    })
}

fn capnp_latest(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("capnp" / "block" / "latest")
        .and(warp::query::query())
        .map(move |params: BlockParams| -> WithStatus<Json> {
            let all = params.all();
            let v = db.fetch_capnp_latest(all).map(|it| it.collect::<Vec<_>>());
            reply::with_status(reply::json(&v), StatusCode::OK)
        })
}

fn libp2p_ipc_latest(
    db: DbCore,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("libp2p_ipc" / "block" / "latest")
        .and(warp::query::query())
        .map(move |params: BlockParams| -> WithStatus<Json> {
            let all = params.all();
            let v = db.fetch_capnp_latest(all).map(|it| it.collect::<Vec<_>>());
            reply::with_status(reply::json(&v), StatusCode::OK)
        })
}

fn firewall_whitelist_set(
    app: Option<Application>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("firewall" / "whitelist" / "enable")
        .and(warp::body::json())
        .and(warp::post())
        .map(move |enable_whitelist| -> WithStatus<Json> {
            if let Some(app) = &app {
                app.enable_firewall(enable_whitelist);
                reply::with_status(reply::json(&()), StatusCode::OK)
            } else {
                reply::with_status(reply::json(&()), StatusCode::NOT_FOUND)
            }
        })
}

fn firewall_whitelist_clear(
    app: Option<Application>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("firewall" / "whitelist" / "disable")
        .and(warp::post())
        .map(move || -> WithStatus<Json> {
            if let Some(app) = &app {
                app.disable_firewall();
                reply::with_status(reply::json(&()), StatusCode::OK)
            } else {
                reply::with_status(reply::json(&()), StatusCode::NOT_FOUND)
            }
        })
}

fn firewall_stats(
    app: Option<Application>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("firewall" / "stats").map(move || -> WithStatus<Json> {
        if let Some(app) = &app {
            let list = app.get_firewall_stats();
            reply::with_status(reply::json(&list), StatusCode::OK)
        } else {
            reply::with_status(reply::json(&()), StatusCode::NOT_FOUND)
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
    app: Option<Application>,
) -> impl Filter<Extract = (impl Reply,), Error = Rejection> + Clone + Sync + Send + 'static {
    use warp::reply::with;

    let cors_filter = warp::cors()
        .allow_any_origin()
        .allow_methods(["OPTIONS", "GET", "POST", "DELETE", "PUT", "HEAD"])
        .allow_credentials(true)
        .allow_headers([
            "Accept",
            "Authorization",
            "baggage",
            "Cache-Control",
            "Content-Type",
            "DNT",
            "If-Modified-Since",
            "Keep-Alive",
            "Origin",
            "sentry-trace",
            "User-Agent",
            "X-Requested-With",
            "X-Cache-Hash",
        ])
        .build();

    let binary = warp::get()
        .and(message_bin(db.clone()))
        .with(with::header("Content-Type", "application/octet-stream"))
        // .with(with::header("Access-Control-Allow-Origin", "*"))
        .with(cors_filter.clone());

    let gets = warp::get().and(
        connection(db.clone())
            .or(connections(db.clone()))
            .or(message(db.clone()))
            .or(message_hex(db.clone()))
            .or(messages(db.clone()))
            .or(stats(db.clone()))
            .or(stats_last(db.clone()))
            .or(stats_latest(db.clone()))
            .or(stats_block_v2(db.clone()))
            .or(stats_block_v2_latest(db.clone()))
            .or(stats_tx(db.clone()))
            .or(stats_tx_latest(db.clone()))
            .or(snark(db.clone()))
            .or(capnp(db.clone()))
            .or(libp2p_ipc(db.clone()))
            .or(capnp_latest(db.clone()))
            .or(libp2p_ipc_latest(db.clone()))
            .or(libp2p_ipc_all(db))
            .or(firewall_stats(app.clone()))
            .or(version().or(openapi())),
    );
    let posts =
        warp::post().and(firewall_whitelist_set(app.clone()).or(firewall_whitelist_clear(app)));

    gets.or(posts)
        .with(with::header("Content-Type", "application/json"))
        // .with(with::header("Access-Control-Allow-Origin", "*"))
        .with(cors_filter)
        .or(binary)
}

pub fn spawn<P, Q, R>(
    port: u16,
    path: P,
    app: Option<Application>,
    key_path: Option<Q>,
    cert_path: Option<R>,
) -> (DbFacade, impl FnOnce(), thread::JoinHandle<()>)
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
    R: AsRef<Path>,
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

    let db = match DbFacade::open(&path) {
        Ok(v) => v,
        Err(err) => {
            log::error!("fatal: {err}");
            process::exit(1);
        }
    };
    log::info!("using db {}", path.as_ref().display());
    let addr = ([0, 0, 0, 0], port);
    let routes = routes(db.core(), app);
    let shutdown = async move {
        rx.await.expect("corresponding sender should exist");
        log::info!("terminating http server...");
    };
    let handle = if let (Some(key_path), Some(cert_path)) = (key_path, cert_path) {
        let (_, server) = warp::serve(routes)
            .tls()
            .key_path(key_path)
            .cert_path(cert_path)
            .bind_with_graceful_shutdown(addr, shutdown);
        thread::spawn(move || rt.block_on(server))
    } else {
        let (_, server) = warp::serve(routes).bind_with_graceful_shutdown(addr, shutdown);
        thread::spawn(move || rt.block_on(server))
    };
    let callback = move || tx.send(()).expect("corresponding receiver should exist");
    (db, callback, handle)
}
