//! Middleware for counting the number of JSON-RPC method calls

use crate::middleware::create_response;
use axum::{
    body::{Body, to_bytes},
    http::Request,
    response::Response,
};
use futures_util::future::BoxFuture;
use opentelemetry::{KeyValue, global, metrics::Counter};
use rpc::Request as RpcRequest;
use std::{
    convert::Infallible,
    task::{Context, Poll},
};
use tower::{Layer, Service};
use tracing::warn;

#[derive(Clone)]
pub struct JsonRpcMethodCounterLayer {
    counter: Counter<u64>,
}

impl Default for JsonRpcMethodCounterLayer {
    fn default() -> Self {
        let meter = global::meter("jsonrpc");
        let counter = meter.u64_counter("jsonrpc_method_calls").build();
        Self { counter }
    }
}

impl<S> Layer<S> for JsonRpcMethodCounterLayer {
    type Service = JsonRpcMethodCounter<S>;
    fn layer(&self, inner: S) -> Self::Service {
        JsonRpcMethodCounter {
            inner,
            counter: self.counter.clone(),
        }
    }
}

#[derive(Clone)]
pub struct JsonRpcMethodCounter<S> {
    inner: S,
    counter: Counter<u64>,
}

impl<S> Service<Request<Body>> for JsonRpcMethodCounter<S>
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
        let counter = self.counter.clone();

        Box::pin(async move {
            let (parts, body) = request.into_parts();

            let request = if let Some(json_rpc) = parts.extensions.get::<RpcRequest>() {
                counter.add(
                    1,
                    &[KeyValue::new("method", json_rpc.method.to_lowercase())],
                );

                Request::from_parts(parts, body)
            } else {
                let bytes = match to_bytes(body, usize::MAX).await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        warn!(%error, middleware = "JsonRpcMethodCounter", "Failed to read request body");
                        return Ok(create_response("Failed to read request body"));
                    }
                };

                if let Ok(rpc_request) = serde_json::from_slice::<RpcRequest>(&bytes) {
                    counter.add(
                        1,
                        &[KeyValue::new("method", rpc_request.method.to_lowercase())],
                    );
                }

                Request::from_parts(parts, Body::from(bytes))
            };

            inner.call(request).await
        })
    }
}
