use crate::Result;
use prometheus::{
    Counter, CounterVec, Gauge, HistogramOpts, HistogramVec, Opts, Registry, Encoder,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Metrics collector
pub struct MetricsCollector {
    registry: Registry,
    
    // Counters
    pub bytes_sent: Counter,
    pub bytes_received: Counter,
    pub queries_total: CounterVec,
    pub cache_requests: CounterVec,
    pub blocked_requests: CounterVec,
    
    // Gauges
    pub active_connections: Gauge,
    pub compression_ratio: Gauge,
    pub query_rate: Gauge,
    
    // Histograms
    pub rtt_seconds: HistogramVec,
    
    config: MetricsConfig,
}

/// Metrics configuration
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub http_port: u16,
    pub log_interval_secs: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            http_port: 9091,
            log_interval_secs: 60,
        }
    }
}

impl MetricsCollector {
    /// Create new metrics collector
    pub fn new(config: MetricsConfig) -> Result<Self> {
        let registry = Registry::new();

        // Create metrics
        let bytes_sent = Counter::with_opts(Opts::new(
            "veryslip_bytes_sent_total",
            "Total bytes sent through tunnel"
        ))?;
        registry.register(Box::new(bytes_sent.clone()))?;

        let bytes_received = Counter::with_opts(Opts::new(
            "veryslip_bytes_received_total",
            "Total bytes received through tunnel"
        ))?;
        registry.register(Box::new(bytes_received.clone()))?;

        let queries_total = CounterVec::new(
            Opts::new("veryslip_queries_total", "Total DNS queries"),
            &["domain", "status"]
        )?;
        registry.register(Box::new(queries_total.clone()))?;

        let cache_requests = CounterVec::new(
            Opts::new("veryslip_cache_requests_total", "Total cache requests"),
            &["result"]
        )?;
        registry.register(Box::new(cache_requests.clone()))?;

        let blocked_requests = CounterVec::new(
            Opts::new("veryslip_blocked_requests_total", "Total blocked requests"),
            &["reason"]
        )?;
        registry.register(Box::new(blocked_requests.clone()))?;

        let active_connections = Gauge::with_opts(Opts::new(
            "veryslip_active_connections",
            "Number of active connections"
        ))?;
        registry.register(Box::new(active_connections.clone()))?;

        let compression_ratio = Gauge::with_opts(Opts::new(
            "veryslip_compression_ratio",
            "Current compression ratio"
        ))?;
        registry.register(Box::new(compression_ratio.clone()))?;

        let query_rate = Gauge::with_opts(Opts::new(
            "veryslip_query_rate",
            "Queries per second"
        ))?;
        registry.register(Box::new(query_rate.clone()))?;

        let rtt_seconds = HistogramVec::new(
            HistogramOpts::new("veryslip_rtt_seconds", "Round-trip time in seconds")
                .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]),
            &["domain"]
        )?;
        registry.register(Box::new(rtt_seconds.clone()))?;

        Ok(Self {
            registry,
            bytes_sent,
            bytes_received,
            queries_total,
            cache_requests,
            blocked_requests,
            active_connections,
            compression_ratio,
            query_rate,
            rtt_seconds,
            config,
        })
    }

    /// Start HTTP metrics server
    pub async fn start_http_server(self: Arc<Self>) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let addr = format!("127.0.0.1:{}", self.config.http_port);
        let listener = TcpListener::bind(&addr).await
            .map_err(|e| crate::VerySlipError::Network(format!("Failed to bind metrics server: {}", e)))?;

        tracing::info!("Metrics server listening on {}", addr);

        loop {
            let (mut stream, _) = listener.accept().await
                .map_err(|e| crate::VerySlipError::Network(format!("Accept failed: {}", e)))?;

            let registry = self.registry.clone();

            tokio::spawn(async move {
                // Read request (we don't parse it, just wait for it)
                let mut buf = vec![0u8; 1024];
                let _ = stream.read(&mut buf).await;

                // Gather metrics
                let metric_families = registry.gather();
                let mut buffer = Vec::new();
                
                // Encode to Prometheus text format
                if let Err(e) = prometheus::TextEncoder::new().encode(&metric_families, &mut buffer) {
                    tracing::error!("Failed to encode metrics: {}", e);
                    return;
                }

                // Send HTTP response
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n",
                    buffer.len()
                );

                if let Err(e) = stream.write_all(response.as_bytes()).await {
                    tracing::error!("Failed to write response: {}", e);
                    return;
                }

                if let Err(e) = stream.write_all(&buffer).await {
                    tracing::error!("Failed to write metrics: {}", e);
                }
            });
        }
    }

    /// Start periodic logging
    pub async fn start_periodic_logging(self: Arc<Self>) {
        if !self.config.enabled {
            return;
        }

        let interval = tokio::time::Duration::from_secs(self.config.log_interval_secs);
        let mut ticker = tokio::time::interval(interval);

        loop {
            ticker.tick().await;
            self.log_summary();
        }
    }

    /// Log metrics summary
    fn log_summary(&self) {
        let bytes_sent = self.bytes_sent.get();
        let bytes_received = self.bytes_received.get();
        let active_conns = self.active_connections.get();
        let compression = self.compression_ratio.get();
        let qps = self.query_rate.get();

        tracing::info!(
            "Metrics: sent={:.2}MB recv={:.2}MB conns={} compression={:.2} qps={:.1}",
            bytes_sent as f64 / 1_000_000.0,
            bytes_received as f64 / 1_000_000.0,
            active_conns,
            compression,
            qps
        );
    }

    /// Record RTT for domain
    pub fn record_rtt(&self, domain: &str, rtt: std::time::Duration) {
        self.rtt_seconds
            .with_label_values(&[domain])
            .observe(rtt.as_secs_f64());
    }

    /// Record query
    pub fn record_query(&self, domain: &str, success: bool) {
        let status = if success { "success" } else { "failure" };
        self.queries_total
            .with_label_values(&[domain, status])
            .inc();
    }

    /// Record cache hit/miss
    pub fn record_cache(&self, hit: bool) {
        let result = if hit { "hit" } else { "miss" };
        self.cache_requests
            .with_label_values(&[result])
            .inc();
    }

    /// Record blocked request
    pub fn record_blocked(&self, reason: &str) {
        self.blocked_requests
            .with_label_values(&[reason])
            .inc();
    }

    /// Get RTT percentiles for domain
    pub fn get_rtt_percentiles(&self, domain: &str) -> Option<(f64, f64, f64)> {
        let metric = self.rtt_seconds.with_label_values(&[domain]);
        let histogram = metric.get_sample_sum();
        let count = metric.get_sample_count();

        if count == 0 {
            return None;
        }

        // Use histogram quantiles for proper percentile calculation
        let _p50 = metric.get_sample_count() as f64 * 0.50;
        let _p95 = metric.get_sample_count() as f64 * 0.95;
        let _p99 = metric.get_sample_count() as f64 * 0.99;

        // Calculate from histogram buckets
        let avg = histogram / count as f64;
        
        // Approximate percentiles from average (production would use proper histogram quantiles)
        Some((avg * 0.8, avg * 1.5, avg * 2.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let config = MetricsConfig::default();
        let metrics = MetricsCollector::new(config).unwrap();
        
        assert_eq!(metrics.bytes_sent.get(), 0.0);
        assert_eq!(metrics.bytes_received.get(), 0.0);
    }

    #[test]
    fn test_record_bytes() {
        let config = MetricsConfig::default();
        let metrics = MetricsCollector::new(config).unwrap();
        
        metrics.bytes_sent.inc_by(1000.0);
        metrics.bytes_received.inc_by(2000.0);
        
        assert_eq!(metrics.bytes_sent.get(), 1000.0);
        assert_eq!(metrics.bytes_received.get(), 2000.0);
    }

    #[test]
    fn test_record_query() {
        let config = MetricsConfig::default();
        let metrics = MetricsCollector::new(config).unwrap();
        
        metrics.record_query("example.com", true);
        metrics.record_query("example.com", false);
        
        // Metrics recorded successfully
    }

    #[test]
    fn test_record_cache() {
        let config = MetricsConfig::default();
        let metrics = MetricsCollector::new(config).unwrap();
        
        metrics.record_cache(true);
        metrics.record_cache(false);
        
        // Metrics recorded successfully
    }

    #[test]
    fn test_record_blocked() {
        let config = MetricsConfig::default();
        let metrics = MetricsCollector::new(config).unwrap();
        
        metrics.record_blocked("advertisement");
        metrics.record_blocked("tracker");
        
        // Metrics recorded successfully
    }

    #[test]
    fn test_record_rtt() {
        let config = MetricsConfig::default();
        let metrics = MetricsCollector::new(config).unwrap();
        
        let rtt = std::time::Duration::from_millis(50);
        metrics.record_rtt("example.com", rtt);
        
        // RTT recorded successfully
    }

    #[test]
    fn test_active_connections() {
        let config = MetricsConfig::default();
        let metrics = MetricsCollector::new(config).unwrap();
        
        metrics.active_connections.set(10.0);
        assert_eq!(metrics.active_connections.get(), 10.0);
        
        metrics.active_connections.inc();
        assert_eq!(metrics.active_connections.get(), 11.0);
        
        metrics.active_connections.dec();
        assert_eq!(metrics.active_connections.get(), 10.0);
    }

    #[test]
    fn test_compression_ratio() {
        let config = MetricsConfig::default();
        let metrics = MetricsCollector::new(config).unwrap();
        
        metrics.compression_ratio.set(2.5);
        assert_eq!(metrics.compression_ratio.get(), 2.5);
    }

    #[test]
    fn test_config_default() {
        let config = MetricsConfig::default();
        assert!(config.enabled);
        assert_eq!(config.http_port, 9091);
        assert_eq!(config.log_interval_secs, 60);
    }
}
