#![forbid(unsafe_code)]

use std::{future::Future, io, pin::pin};

use axum::{Router, body::Body};
use hyper::body::Incoming;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    server::graceful::GracefulShutdown,
    service::TowerToHyperService,
};
use tokio::{net::TcpListener, task};
use tower::ServiceExt as _;

use crate::HttpConfig;

pub async fn serve_with_shutdown<Shutdown>(
    listener: TcpListener,
    router: Router,
    config: &HttpConfig,
    shutdown: Shutdown,
) -> io::Result<()>
where
    Shutdown: Future<Output = ()> + Send + 'static,
{
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(config.header_read_timeout)
        .max_buf_size(config.maximum_header_bytes.max(8 * 1024))
        .max_headers(config.maximum_headers);
    let graceful = GracefulShutdown::new();
    let mut shutdown = pin!(shutdown);

    loop {
        tokio::select! {
            biased;
            () = shutdown.as_mut() => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let service = router.clone().map_request(|request: axum::http::Request<Incoming>| {
                    request.map(Body::new)
                });
                let connection = builder.serve_connection(
                    TokioIo::new(stream),
                    TowerToHyperService::new(service),
                );
                let watched = graceful.watch(connection);
                task::spawn(async move {
                    let _ignored = watched.await;
                });
            }
        }
    }
    drop(listener);
    graceful.shutdown().await;
    Ok(())
}
