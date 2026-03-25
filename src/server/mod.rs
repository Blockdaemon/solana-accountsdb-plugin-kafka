pub mod prom;
pub mod subscriptions;

use {
    bytes::Bytes,
    http::StatusCode,
    http_body_util::Full,
    hyper::{Method, Request, Response, body::Incoming, service::service_fn},
    hyper_util::rt::TokioIo,
    log::*,
    prom::metrics_handler,
    std::{io::Result as IoResult, net::SocketAddr, time::Duration},
    subscriptions::AccountSubscriptions,
    tokio::net::TcpListener,
    tokio::runtime::Runtime,
};

#[derive(Debug)]
pub struct HttpService {
    runtime: Runtime,
}

impl HttpService {
    pub fn new(address: SocketAddr, subs: AccountSubscriptions) -> IoResult<Self> {
        prom::register_metrics();

        let runtime = Runtime::new()?;
        let listener = runtime.block_on(TcpListener::bind(address))?;

        runtime.spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!("Failed to accept connection: {e}");
                        continue;
                    }
                };

                let io = TokioIo::new(stream);
                let subs = subs.clone();

                let service = service_fn(move |req: Request<Incoming>| {
                    let subs = subs.clone();
                    async move {
                        let response = route(req, subs).await;
                        Ok::<_, hyper::Error>(response)
                    }
                });

                tokio::task::spawn(async move {
                    if let Err(err) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .await
                    {
                        error!("Error serving connection: {err}");
                    }
                });
            }
        });
        Ok(HttpService { runtime })
    }

    pub fn shutdown(self) {
        self.runtime.shutdown_timeout(Duration::from_secs(10));
    }
}

async fn route(req: Request<Incoming>, subs: AccountSubscriptions) -> Response<Full<Bytes>> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/metrics") => metrics_handler(),
        (&Method::POST, "/filters/accounts") => {
            subscriptions::handle_post_accounts(req, subs).await
        }
        _ => not_found(),
    }
}

fn not_found() -> Response<Full<Bytes>> {
    match Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Full::new(Bytes::from("")))
    {
        Ok(resp) => resp,
        Err(e) => {
            error!("failed to build not found response: {e}");
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::new(Bytes::new()))
                .unwrap_or_else(|_| {
                    let (mut parts, _) = Response::new(Full::new(Bytes::new())).into_parts();
                    parts.status = StatusCode::NOT_FOUND;
                    Response::from_parts(parts, Full::new(Bytes::new()))
                })
        }
    }
}
