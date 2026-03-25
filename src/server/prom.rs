use {
    crate::version::VERSION as VERSION_INFO,
    bytes::Bytes,
    http_body_util::Full,
    hyper::Response,
    log::*,
    prometheus::{GaugeVec, IntCounterVec, Opts, Registry, TextEncoder},
    rdkafka::{
        client::ClientContext,
        producer::{DeliveryResult, ProducerContext},
        statistics::Statistics,
    },
    std::sync::Once,
};

lazy_static::lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    static ref VERSION: IntCounterVec = IntCounterVec::new(
        Opts::new("version", "Plugin version info"),
        &["key", "value"]
    ).unwrap();

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

    static ref KAFKA_STATS: GaugeVec = GaugeVec::new(
        Opts::new("kafka_stats", "librdkafka metrics"),
        &["broker", "metric"]
    ).unwrap();
}

pub fn register_metrics() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        macro_rules! register {
            ($collector:ident) => {
                REGISTRY
                    .register(Box::new($collector.clone()))
                    .expect("collector can't be registered");
            };
        }
        register!(VERSION);
        register!(UPLOAD_ACCOUNTS_TOTAL);
        register!(UPLOAD_SLOTS_TOTAL);
        register!(UPLOAD_TRANSACTIONS_TOTAL);
        register!(KAFKA_STATS);

        for (key, value) in &[
            ("version", VERSION_INFO.version),
            ("solana", VERSION_INFO.solana),
            ("git", VERSION_INFO.git),
            ("rustc", VERSION_INFO.rustc),
            ("buildts", VERSION_INFO.buildts),
        ] {
            VERSION
                .with_label_values(&[key.to_string(), value.to_string()])
                .inc();
        }
    });
}

pub fn metrics_handler() -> Response<Full<Bytes>> {
    let metrics = TextEncoder::new()
        .encode_to_string(&REGISTRY.gather())
        .unwrap_or_else(|error| {
            error!("could not encode custom metrics: {}", error);
            String::new()
        });
    Response::builder()
        .body(Full::new(Bytes::from(metrics)))
        .unwrap()
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StatsThreadedProducerContext;

impl ClientContext for StatsThreadedProducerContext {
    fn stats(&self, statistics: Statistics) {
        for (name, broker) in statistics.brokers {
            macro_rules! set_value {
                ($name:expr, $value:expr) => {
                    KAFKA_STATS
                        .with_label_values(&[&name.to_string(), &$name.to_string()])
                        .set($value as f64);
                };
            }

            set_value!("outbuf_cnt", broker.outbuf_cnt);
            set_value!("outbuf_msg_cnt", broker.outbuf_msg_cnt);
            set_value!("waitresp_cnt", broker.waitresp_cnt);
            set_value!("waitresp_msg_cnt", broker.waitresp_msg_cnt);
            set_value!("tx", broker.tx);
            set_value!("txerrs", broker.txerrs);
            set_value!("txretries", broker.txretries);
            set_value!("req_timeouts", broker.req_timeouts);

            if let Some(window) = broker.int_latency {
                set_value!("int_latency.min", window.min);
                set_value!("int_latency.max", window.max);
                set_value!("int_latency.avg", window.avg);
                set_value!("int_latency.sum", window.sum);
                set_value!("int_latency.cnt", window.cnt);
                set_value!("int_latency.stddev", window.stddev);
                set_value!("int_latency.hdrsize", window.hdrsize);
                set_value!("int_latency.p50", window.p50);
                set_value!("int_latency.p75", window.p75);
                set_value!("int_latency.p90", window.p90);
                set_value!("int_latency.p95", window.p95);
                set_value!("int_latency.p99", window.p99);
                set_value!("int_latency.p99_99", window.p99_99);
                set_value!("int_latency.outofrange", window.outofrange);
            }

            if let Some(window) = broker.outbuf_latency {
                set_value!("outbuf_latency.min", window.min);
                set_value!("outbuf_latency.max", window.max);
                set_value!("outbuf_latency.avg", window.avg);
                set_value!("outbuf_latency.sum", window.sum);
                set_value!("outbuf_latency.cnt", window.cnt);
                set_value!("outbuf_latency.stddev", window.stddev);
                set_value!("outbuf_latency.hdrsize", window.hdrsize);
                set_value!("outbuf_latency.p50", window.p50);
                set_value!("outbuf_latency.p75", window.p75);
                set_value!("outbuf_latency.p90", window.p90);
                set_value!("outbuf_latency.p95", window.p95);
                set_value!("outbuf_latency.p99", window.p99);
                set_value!("outbuf_latency.p99_99", window.p99_99);
                set_value!("outbuf_latency.outofrange", window.outofrange);
            }
        }
    }
}

impl ProducerContext for StatsThreadedProducerContext {
    type DeliveryOpaque = ();
    fn delivery(&self, _: &DeliveryResult<'_>, _: Self::DeliveryOpaque) {}
}
