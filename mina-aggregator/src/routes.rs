use std::net::SocketAddr;

use serde::Deserialize;
use warp::{
    Filter, Rejection, Reply,
    reply::{WithStatus, Json, self},
    http::StatusCode,
};

use super::database::Database;

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

fn register(
    db: Database,
) -> impl Filter<Extract = (WithStatus<impl Reply>,), Error = Rejection> + Clone + Sync + Send + 'static
{
    #[derive(Deserialize)]
    struct Body {
        alias: String,
        port: u16,
    }

    warp::path!("register")
        .and(warp::addr::remote())
        .and(warp::post())
        .and(warp::body::json())
        .map(move |address: Option<SocketAddr>, Body { alias, port }| {
            log::info!("register {alias}");
            if let Some(mut address) = address {
                address.set_port(port);
                db.register_debugger(alias, address);
            }
            reply::with_status(reply::reply(), StatusCode::OK)
        })
}

fn stats_latest(
    db: Database,
) -> impl Filter<Extract = (WithStatus<Json>,), Error = Rejection> + Clone + Sync + Send + 'static {
    warp::path!("block" / "latest").map(move || -> WithStatus<Json> {
        let v = db.latest();
        reply::with_status(reply::json(&v), StatusCode::OK)
    })
}

pub fn routes(
    database: Database,
) -> impl Filter<Extract = (impl Reply,), Error = Rejection> + Clone + Sync + Send + 'static {
    use warp::reply::with;

    let cors_filter = warp::cors()
        .allow_any_origin()
        .allow_methods(["OPTIONS", "GET", "POST", "DELETE", "PUT"])
        .build();

    let post = warp::post().and(register(database.clone()));
    let get = warp::get().and(version().or(openapi()).or(stats_latest(database)));

    get.or(post)
        .with(with::header("Content-Type", "application/json"))
        .with(with::header("Access-Control-Allow-Origin", "*"))
        .with(cors_filter)
}
