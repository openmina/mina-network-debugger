mod routes;
mod database;

use std::{thread, env};

use tokio::{sync::oneshot, runtime::Runtime};

use self::database::Database;

fn main() {
    env_logger::init();

    let key_path = env::var("HTTPS_KEY_PATH").ok();
    let cert_path = env::var("HTTPS_CERT_PATH").ok();
    let port = env::var("SERVER_PORT")
        .unwrap_or_else(|_| 8000.to_string())
        .parse()
        .unwrap_or(8000);

    let rt = match Runtime::new() {
        Ok(v) => v,
        Err(err) => {
            log::error!("fatal: {err}");
            return;
        }
    };

    let database = Database::default();

    let _guard = rt.enter();
    let (tx, rx) = oneshot::channel();
    let addr = ([0, 0, 0, 0], port);
    let routes = routes::routes(database.clone());
    let shutdown = async move {
        rx.await.expect("corresponding sender should exist");
        log::info!("terminating http server...");
    };
    let server_thread = if let (Some(key_path), Some(cert_path)) = (key_path, cert_path) {
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
    let mut callback = Some(move || tx.send(()).expect("corresponding receiver should exist"));

    let user_handler = move || {
        log::info!("ctrlc");
        callback.take().map(|f| f());
    };
    if let Err(err) = ctrlc::set_handler(user_handler) {
        log::error!("failed to set ctrlc handler {err}");
        return;
    }

    if server_thread.join().is_err() {
        log::error!("server thread panic, this is a bug, must not happen");
    }
    log::info!("terminated");
}
