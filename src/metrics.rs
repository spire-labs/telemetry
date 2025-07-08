//! Metrics module for OpenTelemetry integration.
//!
//! It sets up a meter provider with periodic exporting of metric data.

use eyre::Result;
use global::set_meter_provider;
use opentelemetry::global;
use opentelemetry_otlp::MetricExporter;
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider, Temporality},
};
use tracing::error;

pub struct Metrics;

impl Metrics {
    pub fn init(resource: Resource) -> Result<Self> {
        // TODO: eyre::wrap_err?
        let exporter = MetricExporter::builder()
            .with_temporality(Temporality::default())
            .with_tonic()
            .build()
            .map_err(|error| {
                error!(%error, "Failed to create OTLP Metric exporter");
                error
            })?;

        let reader = PeriodicReader::builder(exporter).build();
        let provider = SdkMeterProvider::builder()
            .with_reader(reader)
            .with_resource(resource)
            .build();

        set_meter_provider(provider);

        Ok(Self)
    }
}
