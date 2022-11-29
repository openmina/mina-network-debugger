mod routes;

use std::{thread, env, sync::{Arc, atomic::{Ordering, AtomicBool}}};

use tokio::{sync::oneshot, runtime::Runtime};

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

    let _guard = rt.enter();
    let (tx, rx) = oneshot::channel();
    let addr = ([0, 0, 0, 0], port);
    let routes = routes::routes();
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
    let callback = move || tx.send(()).expect("corresponding receiver should exist");

    let terminating = Arc::new(AtomicBool::new(false));
    {
        let terminating = terminating.clone();
        let mut callback = Some(callback);
        let user_handler = move || {
            log::info!("ctrlc");
            if let Some(cb) = callback.take() {
                cb();
            }
            terminating.store(true, Ordering::SeqCst);
        };
        if let Err(err) = ctrlc::set_handler(user_handler) {
            log::error!("failed to set ctrlc handler {err}");
            return;
        }
    }

    while !terminating.load(Ordering::SeqCst) {
        thread::yield_now();
    }

    if server_thread.join().is_err() {
        log::error!("server thread panic, this is a bug, must not happen");
    }
    log::info!("terminated");
}
