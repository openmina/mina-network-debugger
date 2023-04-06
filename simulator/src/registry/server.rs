use warp::{hyper::StatusCode, reply, Filter, Rejection, Reply};

pub const PORT: u16 = 80;

pub fn run(nodes: u32) -> anyhow::Result<()> {
    use tokio::{runtime::Builder, sync::oneshot};
    use signal_hook::{consts, iterator::Signals};

    let rt = Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()?;
    let _guard = rt.enter();

    let addr = ([0, 0, 0, 0], PORT);
    let (tx, rx) = oneshot::channel();
    let server = warp::serve(routes(nodes));
    let (_, server) =
        server.bind_with_graceful_shutdown(addr, async { rx.await.unwrap_or_default() });

    let handler = rt.spawn(server);

    let mut signals = Signals::new(&[consts::SIGINT, consts::SIGTERM])?;
    for sig in signals.forever() {
        log::info!("signal {sig}");
        tx.send(()).unwrap_or_default();
        rt.block_on(handler).unwrap();
        break;
    }

    Ok(())
}

fn routes(
    nodes: u32,
) -> impl Filter<Extract = (impl Reply,), Error = Rejection> + Clone + Sync + Send + 'static {
    use std::{
        sync::{Arc, Mutex},
        net::{SocketAddr, IpAddr},
        time::Duration,
        thread,
    };

    use reqwest::blocking::{ClientBuilder, Client};

    use warp::reply::{Json, WithStatus, with};
    use serde::Deserialize;

    use super::state::State;

    #[derive(Deserialize)]
    struct Query {
        build_number: u32,
    }

    #[derive(Deserialize)]
    struct AllowPartialSummary {
        partial: Option<bool>,
    }

    let state = Arc::new(Mutex::new(State::default()));

    let split = warp::path!("split")
        .and(warp::post())
        .map({
            let state = state.clone();

            move || {
                fn enable(client: &Client, peers: Vec<IpAddr>) -> anyhow::Result<()> {
                    let left = peers.iter().take(peers.len() / 2).cloned().collect::<Vec<_>>();
                    let right = peers.iter().skip(peers.len() / 2).cloned().collect::<Vec<_>>();

                    for target in peers.iter() {
                        let whitelist = if left.contains(target) {
                            &left
                        } else {
                            &right
                        };
                        let url = format!("http://{target}:8000/firewall/whitelist");
                        let whitelist = serde_json::to_string(whitelist).unwrap();
                        let _status = client.post(url).body(whitelist).send()?.status();
                    }
                    thread::sleep(Duration::from_secs(30));
                    for target in peers.iter() {
                        let url = format!("http://{target}:8000/firewall/whitelist/clear");
                        let _status = client.post(url).send()?.status();
                    }

                    Ok(())
                }

                let peers = state.lock().expect("must not panic during mutex hold").peers();
                let client = ClientBuilder::new().timeout(Duration::from_secs(10)).build().unwrap();
                thread::spawn(move || {
                    enable(&client, peers).unwrap();
                });
                reply::with_status(reply::json(&""), StatusCode::OK)
            }
        });

    let register = warp::path!("register")
        .and(warp::filters::addr::remote())
        .and(warp::query())
        .map({
            let state = state.clone();
            move |addr, Query { build_number }| -> WithStatus<Json> {
                let Some(addr) = addr else {
                    log::error!("could not determine registrant address");
                    return reply::with_status(
                        reply::json(&"could not determine registrant address"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                };
                log::info!("register {addr}, build {build_number}");
                match state
                    .as_ref()
                    .lock()
                    .expect("must not panic during mutex hold")
                    .register(addr, build_number, nodes)
                {
                    Ok(response) => reply::with_status(reply::json(&response), StatusCode::OK),
                    Err(err) => reply::with_status(
                        reply::json(&err.to_string()),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    ),
                }
            }
        });

    let summary = warp::path!("summary").and(warp::query()).map({
        let state = state.clone();
        move |AllowPartialSummary { partial }| {
            let state = state.lock().expect("must not panic during mutex hold");

            let summary = state.summary();

            if partial.unwrap_or(false) || summary.values().all(|x| x.debugger_report.is_some()) {
                reply::with_status(reply::json(summary), StatusCode::OK)
            } else {
                reply::with_status(reply::json(&None::<()>), StatusCode::OK)
            }
        }
    });

    let net_report = warp::path!("net_report")
        .and(warp::filters::addr::remote())
        .and(warp::body::json())
        .and(warp::query())
        .and(warp::post())
        .map({
            let state = state.clone();
            move |addr: Option<SocketAddr>, report, Query { build_number }| {
                let Some(addr) = addr else {
                    log::error!("could not determine registrant address");
                    return reply::with_status(
                        reply::json(&"could not determine registrant address"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                };
                log::info!("receive net report from {addr}, build {build_number}");
                log::debug!("{report:?}");

                let mut lock = state.lock().expect("must not panic during mutex hold");
                if lock.build_number() != build_number {
                    log::warn!(
                        "ignore net report {build_number}, current {}",
                        lock.build_number()
                    );
                    return reply::with_status(reply::json(&""), StatusCode::GONE);
                }
                lock.add_net_report(addr, report);
                reply::with_status(reply::json(&""), StatusCode::OK)
            }
        });

    let mock_report = warp::path!("mock_report")
        .and(warp::filters::addr::remote())
        .and(warp::body::json())
        .and(warp::query())
        .and(warp::post())
        .map({
            let state = state.clone();
            move |addr: Option<SocketAddr>, report, Query { build_number }| {
                let Some(addr) = addr else {
                    log::error!("could not determine registrant address");
                    return reply::with_status(
                        reply::json(&"could not determine registrant address"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                };
                log::info!("receive node report from {addr}, build {build_number}");
                log::debug!("{report:?}");

                let mut lock = state.lock().expect("must not panic during mutex hold");
                if lock.build_number() != build_number {
                    log::warn!(
                        "ignore node report {build_number}, current {}",
                        lock.build_number()
                    );
                    return reply::with_status(reply::json(&""), StatusCode::GONE);
                }
                lock.add_mock_report(addr, report);
                reply::with_status(reply::json(&""), StatusCode::OK)
            }
        });

        let mock_split_report = warp::path!("mock_split_report")
            .and(warp::filters::addr::remote())
            .and(warp::body::json())
            .and(warp::query())
            .and(warp::post())
            .map({
                let state = state.clone();
                move |addr: Option<SocketAddr>, report, Query { build_number }| {
                    let Some(addr) = addr else {
                        log::error!("could not determine registrant address");
                        return reply::with_status(
                            reply::json(&"could not determine registrant address"),
                            StatusCode::INTERNAL_SERVER_ERROR,
                        );
                    };
                    log::info!("receive node split report from {addr}, build {build_number}");
                    log::debug!("{report:?}");

                    let mut lock = state.lock().expect("must not panic during mutex hold");
                    if lock.build_number() != build_number {
                        log::warn!(
                            "ignore node split report {build_number}, current {}",
                            lock.build_number()
                        );
                        return reply::with_status(reply::json(&""), StatusCode::GONE);
                    }
                    lock.add_mock_split_report(addr, report);

                    reply::with_status(reply::json(&""), StatusCode::OK)
                }
            });

    let debugger_report = warp::path!("report" / "debugger")
        .and(warp::filters::addr::remote())
        .and(warp::body::json())
        .and(warp::query())
        .and(warp::post())
        .map({
            let state = state.clone();
            move |addr: Option<SocketAddr>, report, Query { build_number }| {
                let Some(addr) = addr else {
                    log::error!("could not determine registrant address");
                    return reply::with_status(
                        reply::json(&"could not determine registrant address"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                };
                log::info!("receive debugger report from {addr}, build {build_number}");
                log::debug!("{report:?}");

                let mut lock = state.lock().expect("must not panic during mutex hold");
                if lock.build_number() != build_number {
                    log::warn!(
                        "ignore debugger report {build_number}, current {}",
                        lock.build_number()
                    );
                    return reply::with_status(reply::json(&""), StatusCode::GONE);
                }
                lock.add_debugger_report(addr, report);
                reply::with_status(reply::json(&""), StatusCode::OK)
            }
        });

    let reset = warp::path!("reset").and(warp::post()).map(move || {
        let mut lock = state.lock().expect("must not panic during mutex hold");
        lock.reset();
        reply::with_status(reply::json(&""), StatusCode::OK)
    });

    let get_paths = warp::get().and(register.or(summary));
    let post_paths = warp::post()
        .and(net_report.or(mock_report).or(mock_split_report).or(debugger_report).or(reset).or(split));
    let json_content = with::header("Content-Type", "application/json");

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

    get_paths
        .or(post_paths)
        .with(json_content)
        .with(cors_filter)
}
