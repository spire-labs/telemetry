//! Middleware used to validate incoming HTTP requests.
//!
//! The layer implements a simple JSON-RPC validator which inspects the body and enforces deserialization.
//! The validator does not enforce anything within the body itself as long as it matches the structure
//! therefore "invalid" methods / parameters are allowed as long as deserialization is valid.
//!
//! The validator does not enforce a size on the body therefore usize::MAX number of bytes may be sent
//! which may be a problem for performance / DoS attacks.
//! Other layers may be used to enforce a size limit on the body.

use crate::middleware::create_response;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request},
    response::Response,
};
use futures_util::future::BoxFuture;
use rpc::Request as RpcRequest;
use std::{
    convert::Infallible,
    task::{Context, Poll},
};
use tower::{Layer, Service};
use tracing::{error, warn};

#[derive(Clone)]
pub struct RequestValidationLayer;

impl<S> Layer<S> for RequestValidationLayer {
    type Service = RequestValidator<S>;
    fn layer(&self, inner: S) -> Self::Service {
        RequestValidator { inner }
    }
}

#[derive(Clone)]
pub struct RequestValidator<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for RequestValidator<S>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let (parts, body) = request.into_parts();

            if parts.method != Method::POST {
                // Forward non-POST requests without validation
                let request = Request::from_parts(parts, body);
                return inner.call(request).await;
            }

            let body = match to_bytes(body, usize::MAX).await {
                Ok(body) => body,
                Err(error) => {
                    warn!(%error, middleware = "RequestValidator", "Failed to read request body");
                    return Ok(create_response("Failed to read request body"));
                }
            };

            if let Ok(json_rpc) = serde_json::from_slice::<RpcRequest>(&body) {
                let size = body.len();
                let mut request = Request::from_parts(parts, Body::from(body));

                // Insert deserialized type into extensions to save work in subsequent layers
                request.extensions_mut().insert(json_rpc);
                request.extensions_mut().insert(size);

                let response = match inner.call(request).await {
                    Ok(response) => response,
                    Err(error) => {
                        // Note: Inner service is trait bound to be infallible so this can never happen
                        error!(%error, middleware = "RequestValidator", "Failed to call inner service");
                        return Ok(create_response("Internal server error"));
                    }
                };
                // Note: we forward without modifying the response
                return Ok(response);
            }

            warn!("Request Validation: Invalid JSON-RPC request");
            Ok(create_response("Invalid JSON-RPC request"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, Response, StatusCode},
    };
    use rpc::Response as JsonRpcResponse;
    use serde_json::Value;

    async fn assert_invalid_response(test_request: Body) {
        let mut service = RequestValidationLayer.layer(tower::service_fn(|_req| async {
                Ok(Response::new(Body::from(
                    r#"{"jsonrpc": "2.0", "error": {"code": -32600, "message": "Invalid JSON-RPC request"}, "id": null}"#,
                )))
            }));

        let request = Request::builder()
            .method("POST")
            .uri("/")
            .body(test_request)
            .unwrap();

        let response = service.call(request).await.unwrap();
        let status = response.status();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: JsonRpcResponse<Value> = serde_json::from_slice(&body).unwrap();
        let response = match body {
            JsonRpcResponse::Error(response) => response,
            JsonRpcResponse::Success(_response) => panic!("Expected error response"),
        };

        assert_eq!(status, StatusCode::OK);
        assert_eq!(response.id, Value::Null);
        assert_eq!(response.error.code, -32600);
        assert_eq!(response.error.message, "Invalid JSON-RPC request");
        assert_eq!(response.error.data, None);
    }

    #[tokio::test]
    async fn test_valid_request() {
        let mut service = RequestValidationLayer.layer(tower::service_fn(|_req| async {
            Ok(Response::new(Body::from(
                r#"{"jsonrpc": "2.0", "result": "0x1234", "id": 1}"#,
            )))
        }));

        let valid_request =
            r#"{"jsonrpc": "2.0", "method": "eth_blockNumber", "params": [], "id": 1}"#;

        let request = Request::builder()
            .method("POST")
            .uri("/")
            .body(Body::from(valid_request))
            .unwrap();

        let response = service.call(request).await.unwrap();
        let status = response.status();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: JsonRpcResponse<Value> = serde_json::from_slice(&body).unwrap();
        let response = match body {
            JsonRpcResponse::Error(_response) => panic!("Expected success response"),
            JsonRpcResponse::Success(response) => response,
        };

        assert_eq!(status, StatusCode::OK);
        assert_eq!(response.jsonrpc, Value::String("2.0".to_string()));
        assert_eq!(response.id, Value::Number(1.into()));
        assert_eq!(response.result, Value::String("0x1234".to_string()));
    }

    #[tokio::test]
    async fn test_invalid_request() {
        let test_request = Body::from(r#"{"invalid": "json"}"#);
        assert_invalid_response(test_request).await;
    }

    #[tokio::test]
    async fn test_malformed_request() {
        let test_request =
            Body::from(r#"{"jsonrpc": "2.0", "method": "eth_blockNumber", "params": [], "id": 1"#);
        assert_invalid_response(test_request).await;
    }

    #[tokio::test]
    async fn test_empty_body_request() {
        let test_request = Body::empty();
        assert_invalid_response(test_request).await;
    }
}
