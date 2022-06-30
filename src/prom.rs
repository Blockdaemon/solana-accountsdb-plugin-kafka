use {
    hyper::{
        server::conn::AddrStream,
        service::{make_service_fn, service_fn},
        Body, Request, Response, Server, StatusCode,
    },
    log::*,
    prometheus::{IntCounterVec, Opts, Registry, TextEncoder},
    std::{io::Result as IoResult, net::SocketAddr, sync::Once, time::Duration},
    tokio::runtime::Runtime,
};

lazy_static::lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    pub static ref UPLOAD_ACCOUNTS_TOTAL: IntCounterVec = IntCounterVec::new(
        Opts::new("upload_accounts_total", "Status of uploaded accounts"),
        &["status"]
    ).unwrap();

    pub static ref UPLOAD_SLOTS_TOTAL: IntCounterVec = IntCounterVec::new(
        Opts::new("upload_slots_total", "Status of uploaded slots"),
        &["status"]
    ).unwrap();

    pub static ref UPLOAD_TRANSACTIONS_TOTAL: IntCounterVec = IntCounterVec::new(
        Opts::new("upload_transactions_total", "Status of uploaded transactions"),
        &["status"]
    ).unwrap();
}

#[derive(Debug)]
pub struct PrometheusService {
    runtime: Runtime,
}

impl PrometheusService {
    pub fn new(address: SocketAddr) -> IoResult<Self> {
        static REGISTER: Once = Once::new();
        REGISTER.call_once(|| {
            macro_rules! register {
                ($collector:ident) => {
                    REGISTRY
                        .register(Box::new($collector.clone()))
                        .expect("collector can't be registered");
                };
            }
            register!(UPLOAD_ACCOUNTS_TOTAL);
            register!(UPLOAD_SLOTS_TOTAL);
            register!(UPLOAD_TRANSACTIONS_TOTAL);
        });

        let runtime = Runtime::new()?;
        runtime.spawn(async move {
            let make_service = make_service_fn(move |_: &AddrStream| async move {
                Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| async move {
                    let response = match req.uri().path() {
                        "/metrics" => metrics_handler(),
                        _ => not_found_handler(),
                    };
                    Ok::<_, hyper::Error>(response)
                }))
            });
            if let Err(error) = Server::bind(&address).serve(make_service).await {
                error!("prometheus service failed: {}", error);
            }
        });
        Ok(PrometheusService { runtime })
    }

    pub fn shutdown(self) {
        self.runtime.shutdown_timeout(Duration::from_secs(10));
    }
}

fn metrics_handler() -> Response<Body> {
    let metrics = TextEncoder::new()
        .encode_to_string(&REGISTRY.gather())
        .unwrap_or_else(|error| {
            error!("could not encode custom metrics: {}", error);
            String::new()
        });
    Response::builder().body(Body::from(metrics)).unwrap()
}

fn not_found_handler() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::empty())
        .unwrap()
}
