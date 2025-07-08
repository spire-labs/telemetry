//! Middleware for recording JSON-RPC method body size and latency

use crate::middleware::create_response;
use axum::{
    body::{Body, to_bytes},
    http::Request,
    response::Response,
};
use futures_util::future::BoxFuture;
use opentelemetry::{KeyValue, global, metrics::Histogram};
use rpc::Request as RpcRequest;
use std::{
    convert::Infallible,
    task::{Context, Poll},
    time::Instant,
};
use tower::{Layer, Service};
use tracing::warn;

#[derive(Clone)]
pub struct JsonRpcMethodHistogramLayer {
    size: Histogram<u64>,
    latency: Histogram<u64>,
}

impl Default for JsonRpcMethodHistogramLayer {
    fn default() -> Self {
        let meter = global::meter("jsonrpc");
        let size = meter.u64_histogram("jsonrpc_method_body_size").build();
        let latency = meter.u64_histogram("jsonrpc_method_latency_ms").build();
        Self { size, latency }
    }
}

impl<S> Layer<S> for JsonRpcMethodHistogramLayer {
    type Service = JsonRpcMethodHistogram<S>;
    fn layer(&self, inner: S) -> Self::Service {
        JsonRpcMethodHistogram {
            inner,
            size: self.size.clone(),
            latency: self.latency.clone(),
        }
    }
}

#[derive(Clone)]
pub struct JsonRpcMethodHistogram<S> {
    inner: S,
    size: Histogram<u64>,
    latency: Histogram<u64>,
}

impl<S> Service<Request<Body>> for JsonRpcMethodHistogram<S>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let size = self.size.clone();
        let latency = self.latency.clone();

        Box::pin(async move {
            let start = Instant::now();
            let (parts, body) = request.into_parts();

            let (request, method) = if let Some(json_rpc) = parts.extensions.get::<RpcRequest>() {
                (
                    {
                        if let Some(bytes_size) = parts.extensions.get::<usize>() {
                            size.record(
                                *bytes_size as u64,
                                &[KeyValue::new("method", json_rpc.method.to_lowercase())],
                            );
                        }
                        Request::from_parts(parts.clone(), body)
                    },
                    Some(json_rpc.method.to_lowercase()),
                )
            } else {
                let bytes = match to_bytes(body, usize::MAX).await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        warn!(%error, middleware = "JsonRpcMethodMetrics", "Failed to read request body");
                        return Ok(create_response("Failed to read request body"));
                    }
                };

                let method = if let Ok(json_rpc) = serde_json::from_slice::<RpcRequest>(&bytes) {
                    size.record(
                        bytes.len() as u64,
                        &[KeyValue::new("method", json_rpc.method.to_lowercase())],
                    );

                    Some(json_rpc.method.to_lowercase())
                } else {
                    None
                };

                (Request::from_parts(parts, Body::from(bytes)), method)
            };

            let response = inner.call(request).await;
            let elapsed_ms = start.elapsed().as_millis() as u64;

            if let Some(method) = method {
                latency.record(elapsed_ms, &[KeyValue::new("method", method)]);
            }

            response
        })
    }
}
