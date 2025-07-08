use axum::{
    body::{Body, Bytes},
    http::{Request, Response},
};
use std::time::Duration;
use tower_http::{
    classify::{ServerErrorsAsFailures, ServerErrorsFailureClass, SharedClassifier},
    request_id::RequestId,
    trace::{DefaultOnEos, TraceLayer},
};
use tracing::{Span, error, info, info_span};

#[allow(clippy::type_complexity)]
pub fn trace_layer() -> TraceLayer<
    SharedClassifier<ServerErrorsAsFailures>,
    impl Fn(&Request<Body>) -> Span + Clone,
    impl Fn(&Request<Body>, &Span) + Clone,
    impl Fn(&Response<Body>, Duration, &Span) + Clone,
    impl Fn(&Bytes, Duration, &Span) + Clone,
    DefaultOnEos,
    impl Fn(ServerErrorsFailureClass, Duration, &Span) + Clone,
> {
    TraceLayer::new_for_http()
        .make_span_with(|request: &Request<Body>| {
            let trace_id = request
                .extensions()
                .get::<RequestId>()
                .map(|id| id.header_value().to_str().unwrap_or("none").to_string())
                .unwrap_or_else(|| "none".into());

            info_span!(
                "http_request",
                trace_id,
                method     = %request.method(),
                uri        = %request.uri().path(),
            )
        })
        .on_request(|_request: &Request<Body>, span: &Span| {
            info!(parent: span, "Incoming request");
        })
        .on_body_chunk(|chunk: &Bytes, _latency: Duration, span: &Span| {
            info!(parent: span, bytes = chunk.len(), "Body chunk");
        })
        .on_response(
            |response: &Response<Body>, latency: Duration, span: &Span| {
                info!(
                    parent: span,
                    status      = response.status().as_u16(),
                    latency_ms  = latency.as_millis(),
                    "Request Succeeded"
                )
            },
        )
        .on_failure(
            |class: ServerErrorsFailureClass, latency: Duration, span: &Span| {
                let (error, status) = match class {
                    ServerErrorsFailureClass::StatusCode(code) => {
                        ("N/A".to_string(), code.as_u16())
                    }
                    ServerErrorsFailureClass::Error(error) => (error.to_string(), 500),
                };

                error!(
                    parent: span,
                    error,
                    status,
                    latency_ms  = latency.as_millis(),
                    "Request Failed"
                )
            },
        )
}
