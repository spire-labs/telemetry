mod histogram;
mod method_counter;
mod request_validation;
mod tracing;

use axum::{
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
pub use histogram::JsonRpcMethodHistogramLayer;
pub use method_counter::JsonRpcMethodCounterLayer;
pub use request_validation::RequestValidationLayer;
use rpc::{ErrorBody, Response as JsonRpcResponse, code::INVALID_REQUEST};
use serde_json::Value;
pub use tracing::trace_layer;

pub fn create_response(message: &str) -> Response {
    let response =
        JsonRpcResponse::<Value>::error(ErrorBody::new(INVALID_REQUEST, message), Value::Null);

    let body = match serde_json::to_vec(&response) {
        Ok(body) => body,
        Err(_) => b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32600,\"message\":\"Invalid JSON-RPC request\"}}".to_vec(),
    };

    // Hardcode the unwrap as a last effort in case the body contributed to the error
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::from(
            "{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32600,\"message\":\"Invalid JSON-RPC request\"}}"
        )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn test_create_response() {
        let response = create_response("Test error message");
        let status = response.status();
        let headers = response.headers().clone();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: JsonRpcResponse<Value> = serde_json::from_slice(&body).unwrap();
        let response = match body {
            JsonRpcResponse::Error(response) => response,
            JsonRpcResponse::Success(_response) => panic!("Expected error response"),
        };

        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
        assert_eq!(response.id, Value::Null);
        assert_eq!(response.error.code, -32600);
        assert_eq!(response.error.message, "Test error message");
        assert_eq!(response.error.data, None);
    }
}
