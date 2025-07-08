//! Telemetry module for OpenTelemetry integration.
//!
//! It sets up tracing and metrics collection using OTLP exporters.

mod metrics;
pub mod middleware;
mod tracing;

use eyre::Result;
use metrics::Metrics;
use opentelemetry::{KeyValue, Value};
use opentelemetry_sdk::Resource;
use std::env;
use tracing::Tracing;

pub struct Telemetry {
    _tracing: Tracing,
    _metrics: Metrics,
}

impl Telemetry {
    pub fn init(name: impl Into<Value>) -> Result<Self> {
        let resource = Resource::builder()
            .with_service_name(name)
            .with_attributes(vec![
                KeyValue::new(
                    "service.commit",
                    env::var("GITHUB_SHA").unwrap_or_else(|_| "dev".to_string()),
                ),
                KeyValue::new(
                    "service.environment",
                    env::var("ENVIRONMENT").unwrap_or_else(|_| "dev".to_string()),
                ),
            ])
            .build();

        Ok(Self {
            _tracing: Tracing::init(resource.clone())?,
            _metrics: Metrics::init(resource)?,
        })
    }
}
