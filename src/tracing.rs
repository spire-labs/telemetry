//! Tracing module for OpenTelemetry integration.

use eyre::{Result, eyre};
use fmt::layer;
use global::{set_text_map_propagator, set_tracer_provider};
use opentelemetry::{global, trace::TracerProvider};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, SpanExporter};
use opentelemetry_sdk::{
    Resource, logs::SdkLoggerProvider, propagation::TraceContextPropagator,
    trace::TracerProviderBuilder,
};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, registry, util::SubscriberInitExt};

pub struct Tracing;

impl Tracing {
    pub fn init(resource: Resource) -> Result<Self> {
        let console_logger = layer()
            .json()
            .with_current_span(true)
            .flatten_event(true)
            .with_target(true)
            .with_span_list(false);

        let log_exporter = LogExporter::builder().with_tonic().build()?;

        let logger_provider = SdkLoggerProvider::builder()
            .with_batch_exporter(log_exporter)
            .with_resource(resource.clone())
            .build();

        let otel_logger = OpenTelemetryTracingBridge::new(&logger_provider);

        // TODO: eyre::wrap_err?
        let span_exporter = SpanExporter::builder()
            .with_tonic()
            .build()
            .map_err(|e| eyre!("Failed to build SpanExporter: {:?}", e))?;

        let tracer_provider = TracerProviderBuilder::default()
            .with_batch_exporter(span_exporter)
            .with_resource(resource.clone())
            .build();

        set_tracer_provider(tracer_provider.clone());
        set_text_map_propagator(TraceContextPropagator::new());

        let tracer = tracer_provider.tracer("otel-spans");
        let otel_tracer = OpenTelemetryLayer::new(tracer);

        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

        registry()
            .with(env_filter)
            .with(otel_logger)
            .with(otel_tracer)
            .with(console_logger)
            .init();

        Ok(Self)
    }
}
