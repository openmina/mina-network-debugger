use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    thread,
};

use signal_hook::{consts, iterator::Signals};
use tokio::{runtime::Runtime, sync::oneshot};
use warp::{
    hyper::StatusCode,
    reply::{self, Json, WithStatus},
    Filter, Rejection, Reply,
};

use super::{constants, test_state::State};

pub fn run() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let _guard = rt.enter();

    let addr = ([0, 0, 0, 0], constants::CENTER_PORT);
    let (tx, rx) = oneshot::channel();
    let state = Arc::new(Mutex::new(State::new(std::env::var("MY_POD_IP")?.parse()?)));
    let (_, server) = warp::serve(routes(state))
        .bind_with_graceful_shutdown(addr, async { rx.await.unwrap_or_default() });

    let handler = thread::spawn(move || rt.block_on(server));

    let mut signals = Signals::new(&[consts::SIGINT, consts::SIGTERM])?;
    for sig in signals.forever() {
        log::info!("signal {sig}");
        tx.send(()).unwrap_or_default();
        handler.join().unwrap();
        break;
    }

    Ok(())
}

fn routes(
    state: Arc<Mutex<State>>,
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

    let g = register(state.clone())
        .or(summary(state.clone()))
        .or(test(state.clone()));
    let p = report(state.clone())
        .or(debugger_report(state.clone()))
        .or(net_report(state.clone()))
        .or(reset(state));
    warp::get()
        .and(g)
        .or(warp::post().and(p))
        .with(with::header("Content-Type", "application/json"))
        .with(cors_filter)
}

fn reset(
    state: Arc<Mutex<State>>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("reset").and(warp::post()).map(move || {
        state
            .lock()
            .expect("must not panic during mutex hold")
            .reset();
        reply::with_status(reply::json(&1), StatusCode::OK)
    })
}

#[derive(serde::Deserialize)]
struct Query {
    build_number: u32,
}

fn register(
    state: Arc<Mutex<State>>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("register")
        .and(warp::filters::addr::remote())
        .and(warp::query())
        .map(move |addr, Query { build_number }| -> WithStatus<Json> {
            let Some(addr) = addr else {
                log::error!("could not determine registrant address");
                return reply::with_status(
                    reply::json(&"could not determine registrant address"),
                    StatusCode::INTERNAL_SERVER_ERROR,
                );
            };
            log::debug!("register {addr}, build {build_number}");
            match state
                .lock()
                .expect("must not panic during mutex hold")
                .register(addr, build_number)
            {
                Ok(response) => reply::with_status(reply::json(&response), StatusCode::OK),
                Err(err) => reply::with_status(
                    reply::json(&err.to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR,
                ),
            }
        })
}

fn net_report(
    state: Arc<Mutex<State>>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("net_report")
        .and(warp::filters::addr::remote())
        .and(warp::body::json())
        .and(warp::query())
        .and(warp::post())
        .map(
            move |addr: Option<SocketAddr>, report, Query { build_number }| {
                let Some(addr) = addr else {
                    log::error!("could not determine registrant address");
                    return reply::with_status(
                        reply::json(&"could not determine registrant address"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                };
                log::debug!("receive net report from {addr}, build {build_number}");
                log::debug!("{report:?}");

                let mut lock = state.lock().expect("must not panic during mutex hold");
                if lock.build_number() != build_number {
                    log::debug!(
                        "ignore net report {build_number}, current {}",
                        lock.build_number()
                    );
                    return reply::with_status(reply::json(&""), StatusCode::GONE);
                }
                lock.add_net_report(addr, report);
                reply::with_status(reply::json(&""), StatusCode::OK)
            },
        )
}

fn report(
    state: Arc<Mutex<State>>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("report" / "node")
        .and(warp::filters::addr::remote())
        .and(warp::body::json())
        .and(warp::query())
        .and(warp::post())
        .map(
            move |addr: Option<SocketAddr>, report, Query { build_number }| {
                let Some(addr) = addr else {
                    log::error!("could not determine registrant address");
                    return reply::with_status(
                        reply::json(&"could not determine registrant address"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                };
                log::debug!("receive node report from {addr}, build {build_number}");
                log::debug!("{report:?}");

                let mut lock = state.lock().expect("must not panic during mutex hold");
                if lock.build_number() != build_number {
                    log::debug!(
                        "ignore node report {build_number}, current {}",
                        lock.build_number()
                    );
                    return reply::with_status(reply::json(&""), StatusCode::GONE);
                }
                lock.add_node_report(addr, report);
                reply::with_status(reply::json(&""), StatusCode::OK)
            },
        )
}

fn debugger_report(
    state: Arc<Mutex<State>>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("report" / "debugger")
        .and(warp::filters::addr::remote())
        .and(warp::body::json())
        .and(warp::query())
        .and(warp::post())
        .map(
            move |addr: Option<SocketAddr>, report, Query { build_number }| {
                let Some(addr) = addr else {
                    log::error!("could not determine registrant address");
                    return reply::with_status(
                        reply::json(&"could not determine registrant address"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                };
                log::debug!("receive debugger report from {addr}, build {build_number}");
                log::debug!("{report:?}");

                let mut lock = state.lock().expect("must not panic during mutex hold");
                if lock.build_number() != build_number {
                    log::debug!(
                        "ignore debugger report {build_number}, current {}",
                        lock.build_number()
                    );
                    return reply::with_status(reply::json(&""), StatusCode::GONE);
                }
                lock.add_debugger_report(addr, report);
                reply::with_status(reply::json(&""), StatusCode::OK)
            },
        )
}

fn summary(
    state: Arc<Mutex<State>>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("summary").map(move || {
        let response = &state
            .lock()
            .expect("must not panic during mutex hold")
            .summary;
        reply::with_status(reply::json(response), StatusCode::OK)
    })
}

fn test(
    state: Arc<Mutex<State>>,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("test").map(move || {
        let r = &state
            .lock()
            .expect("must not panic during mutex hold")
            .test_result;
        reply::with_status(reply::json(r), StatusCode::OK)
    })
}
