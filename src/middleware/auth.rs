//! Middleware helpers for authenticated JSON-RPC requests.
//!
//! This layer buffers JSON-RPC bodies (already validated upstream), authenticates
//! Flashbots-style signatures from the `X-Flashbots-Signature` header, and stores
//! the recovered signer in request extensions for downstream handlers.

use crate::middleware::{create_response, with_metadata};
use alloy::primitives::{Address, Signature, eip191_hash_message, keccak256};
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request},
    response::Response,
};
use futures_util::future::BoxFuture;
use std::{
    convert::Infallible,
    str::FromStr,
    task::{Context, Poll},
};
use thiserror::Error;
use tower::{Layer, Service};
use tracing::{error, warn};

const SIGNATURE_HEADER: &str = "X-Flashbots-Signature";

#[derive(Clone, Default)]
pub struct AuthenticatedJsonRpcLayer;

impl<S> Layer<S> for AuthenticatedJsonRpcLayer {
    type Service = AuthenticatedJsonRpc<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthenticatedJsonRpc { inner }
    }
}

#[derive(Clone)]
pub struct AuthenticatedJsonRpc<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for AuthenticatedJsonRpc<S>
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

            let raw_body = match to_bytes(body, usize::MAX).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    warn!(%error, middleware = "AuthenticatedJsonRpc", "Failed to read request body");
                    return Ok(create_response("Failed to read request body"));
                }
            };

            let signer = match authenticate_signature(&parts.headers, &raw_body) {
                Ok(Some(address)) => Some(address),
                Ok(None) => None,
                Err(error) => {
                    warn!(%error, middleware = "AuthenticatedJsonRpc", "Failed to authenticate request");
                    return Ok(create_response(&error.to_string()));
                }
            };

            let mut request = Request::from_parts(parts, Body::from(raw_body));

            // Store signer in shared metadata for downstream use.
            with_metadata(request.extensions_mut(), |metadata| {
                metadata.signer = signer;
            });

            inner.call(request).await
        })
    }
}

#[derive(Debug, Error)]
enum AuthenticatedJsonRpcError {
    #[error("Invalid X-Flashbots-Signature header encoding")]
    InvalidSignatureHeaderEncoding,
    #[error("Invalid X-Flashbots-Signature header format")]
    MalformedSignatureHeader,
    #[error("Invalid signer address")]
    InvalidSignerAddress,
    #[error("Invalid signature encoding")]
    InvalidSignatureEncoding,
    #[error("Unable to recover signer from signature")]
    SignatureRecoveryFailed,
    #[error("Signature does not match claimed address (possible body tampering)")]
    SignatureMismatch,
}

fn authenticate_signature(
    headers: &HeaderMap,
    raw_body: &[u8],
) -> Result<Option<Address>, AuthenticatedJsonRpcError> {
    let header_value = match headers.get(SIGNATURE_HEADER) {
        Some(value) => value,
        None => return Ok(None),
    };

    let header_str = header_value
        .to_str()
        .map_err(|_| AuthenticatedJsonRpcError::InvalidSignatureHeaderEncoding)?;

    let (address_hex, signature_hex) = header_str
        .split_once(':')
        .ok_or(AuthenticatedJsonRpcError::MalformedSignatureHeader)?;

    let claimed_address = Address::from_str(address_hex.trim())
        .map_err(|_| AuthenticatedJsonRpcError::InvalidSignerAddress)?;

    let signature = Signature::from_str(signature_hex.trim())
        .map_err(|_| AuthenticatedJsonRpcError::InvalidSignatureEncoding)?;

    let body_hash = keccak256(raw_body);
    let eth_message_hash = eip191_hash_message(body_hash);
    let recovered_address = signature
        .recover_address_from_prehash(&eth_message_hash)
        .map_err(|_| AuthenticatedJsonRpcError::SignatureRecoveryFailed)?;

    if recovered_address != claimed_address {
        return Err(AuthenticatedJsonRpcError::SignatureMismatch);
    }

    Ok(Some(recovered_address))
}
