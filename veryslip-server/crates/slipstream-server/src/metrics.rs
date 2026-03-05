/// Metrics collection module for Prometheus integration
/// 
/// Tracks server performance metrics including:
/// - Bytes sent/received
/// - Query counts (total and batched)
/// - Active connections and streams
/// - Compression statistics
/// - RTT histogram
/// - Per-domain statistics

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// RTT histogram buckets (in milliseconds)
const RTT_BUCKETS: &[u64] = &[10, 50, 100, 500, 1000, u64::MAX];

/// Per-domain statistics
#[derive(Debug, Default)]
pub struct DomainStats {
    /// Number of queries for this domain
    pub queries: AtomicU64,
}

impl DomainStats {
    pub fn new() -> Self {
        Self::default()
    }
}

/// RTT histogram with fixed buckets
#[derive(Debug)]
pub struct RttHistogram {
    /// Bucket counts: [0-10ms, 10-50ms, 50-100ms, 100-500ms, 500-1000ms, 1000ms+]
    buckets: [AtomicU64; 6],
}

impl RttHistogram {
    pub fn new() -> Self {
        Self {
            buckets: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }
    
    /// Record an RTT sample (in milliseconds)
    pub fn record(&self, rtt_ms: u64) {
        for (i, &bucket_limit) in RTT_BUCKETS.iter().enumerate() {
            if rtt_ms < bucket_limit {
                self.buckets[i].fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }
    
    /// Get bucket counts
    pub fn buckets(&self) -> [u64; 6] {
        [
            self.buckets[0].load(Ordering::Relaxed),
            self.buckets[1].load(Ordering::Relaxed),
            self.buckets[2].load(Ordering::Relaxed),
            self.buckets[3].load(Ordering::Relaxed),
            self.buckets[4].load(Ordering::Relaxed),
            self.buckets[5].load(Ordering::Relaxed),
        ]
    }
}

impl Default for RttHistogram {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics collector for server statistics
pub struct MetricsCollector {
    // Traffic metrics
    bytes_sent: AtomicU64,
    bytes_received: AtomicU64,
    
    // Query metrics
    queries_processed: AtomicU64,
    queries_batched: AtomicU64,
    
    // Connection metrics
    active_connections: AtomicU64,
    active_streams: AtomicU64,
    
    // Compression metrics
    compression_bytes_in: AtomicU64,
    compression_bytes_out: AtomicU64,
    
    // RTT histogram
    rtt_histogram: RttHistogram,
    
    // Per-domain statistics
    domain_stats: Arc<RwLock<HashMap<String, Arc<DomainStats>>>>,
}

impl MetricsCollector {
    /// Create new metrics collector
    pub fn new() -> Self {
        Self {
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            queries_processed: AtomicU64::new(0),
            queries_batched: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            active_streams: AtomicU64::new(0),
            compression_bytes_in: AtomicU64::new(0),
            compression_bytes_out: AtomicU64::new(0),
            rtt_histogram: RttHistogram::new(),
            domain_stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Record bytes sent
    pub fn record_bytes_sent(&self, bytes: u64) {
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }
    
    /// Record bytes received
    pub fn record_bytes_received(&self, bytes: u64) {
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }
    
    /// Record a query processed
    pub fn record_query(&self, domain: &str, is_batched: bool) {
        self.queries_processed.fetch_add(1, Ordering::Relaxed);
        
        if is_batched {
            self.queries_batched.fetch_add(1, Ordering::Relaxed);
        }
        
        // Update per-domain statistics
        let stats = {
            let domain_stats = self.domain_stats.read().unwrap();
            domain_stats.get(domain).cloned()
        };
        
        if let Some(stats) = stats {
            stats.queries.fetch_add(1, Ordering::Relaxed);
        } else {
            // Create new domain stats
            let mut domain_stats = self.domain_stats.write().unwrap();
            let stats = Arc::new(DomainStats::new());
            stats.queries.fetch_add(1, Ordering::Relaxed);
            domain_stats.insert(domain.to_string(), stats);
        }
    }
    
    /// Record compression statistics
    pub fn record_compression(&self, bytes_in: u64, bytes_out: u64) {
        self.compression_bytes_in.fetch_add(bytes_in, Ordering::Relaxed);
        self.compression_bytes_out.fetch_add(bytes_out, Ordering::Relaxed);
    }
    
    /// Record RTT sample (in milliseconds)
    pub fn record_rtt(&self, rtt_ms: u64) {
        self.rtt_histogram.record(rtt_ms);
    }
    
    /// Set active connections count
    pub fn set_active_connections(&self, count: u64) {
        self.active_connections.store(count, Ordering::Relaxed);
    }
    
    /// Set active streams count
    pub fn set_active_streams(&self, count: u64) {
        self.active_streams.store(count, Ordering::Relaxed);
    }
    
    /// Get bytes sent
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent.load(Ordering::Relaxed)
    }
    
    /// Get bytes received
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received.load(Ordering::Relaxed)
    }
    
    /// Get queries processed
    pub fn queries_processed(&self) -> u64 {
        self.queries_processed.load(Ordering::Relaxed)
    }
    
    /// Get queries batched
    pub fn queries_batched(&self) -> u64 {
        self.queries_batched.load(Ordering::Relaxed)
    }
    
    /// Get active connections
    pub fn active_connections(&self) -> u64 {
        self.active_connections.load(Ordering::Relaxed)
    }
    
    /// Get active streams
    pub fn active_streams(&self) -> u64 {
        self.active_streams.load(Ordering::Relaxed)
    }
    
    /// Get compression ratio (1.0 - bytes_out/bytes_in)
    pub fn compression_ratio(&self) -> f64 {
        let bytes_in = self.compression_bytes_in.load(Ordering::Relaxed) as f64;
        let bytes_out = self.compression_bytes_out.load(Ordering::Relaxed) as f64;
        
        if bytes_in == 0.0 {
            return 0.0;
        }
        
        1.0 - (bytes_out / bytes_in)
    }
    
    /// Get RTT histogram buckets
    pub fn rtt_histogram_buckets(&self) -> [u64; 6] {
        self.rtt_histogram.buckets()
    }
    
    /// Get per-domain statistics snapshot
    pub fn domain_stats_snapshot(&self) -> HashMap<String, u64> {
        let domain_stats = self.domain_stats.read().unwrap();
        domain_stats
            .iter()
            .map(|(domain, stats)| {
                (domain.clone(), stats.queries.load(Ordering::Relaxed))
            })
            .collect()
    }
    
    /// Export metrics in Prometheus text format
    pub fn export_prometheus(&self) -> String {
        let mut output = String::new();
        
        // Bytes sent
        output.push_str("# HELP veryslip_bytes_sent_total Total bytes sent\n");
        output.push_str("# TYPE veryslip_bytes_sent_total counter\n");
        output.push_str(&format!("veryslip_bytes_sent_total {}\n", self.bytes_sent()));
        output.push('\n');
        
        // Bytes received
        output.push_str("# HELP veryslip_bytes_received_total Total bytes received\n");
        output.push_str("# TYPE veryslip_bytes_received_total counter\n");
        output.push_str(&format!("veryslip_bytes_received_total {}\n", self.bytes_received()));
        output.push('\n');
        
        // Queries processed
        output.push_str("# HELP veryslip_queries_total Total queries processed\n");
        output.push_str("# TYPE veryslip_queries_total counter\n");
        output.push_str(&format!("veryslip_queries_total {}\n", self.queries_processed()));
        output.push('\n');
        
        // Queries batched
        output.push_str("# HELP veryslip_queries_batched_total Total batched queries processed\n");
        output.push_str("# TYPE veryslip_queries_batched_total counter\n");
        output.push_str(&format!("veryslip_queries_batched_total {}\n", self.queries_batched()));
        output.push('\n');
        
        // Active connections
        output.push_str("# HELP veryslip_active_connections Current number of active connections\n");
        output.push_str("# TYPE veryslip_active_connections gauge\n");
        output.push_str(&format!("veryslip_active_connections {}\n", self.active_connections()));
        output.push('\n');
        
        // Active streams
        output.push_str("# HELP veryslip_active_streams Current number of active streams\n");
        output.push_str("# TYPE veryslip_active_streams gauge\n");
        output.push_str(&format!("veryslip_active_streams {}\n", self.active_streams()));
        output.push('\n');
        
        // Compression ratio
        output.push_str("# HELP veryslip_compression_ratio Current compression ratio\n");
        output.push_str("# TYPE veryslip_compression_ratio gauge\n");
        output.push_str(&format!("veryslip_compression_ratio {:.4}\n", self.compression_ratio()));
        output.push('\n');
        
        // RTT histogram
        output.push_str("# HELP veryslip_rtt_seconds RTT histogram\n");
        output.push_str("# TYPE veryslip_rtt_seconds histogram\n");
        let buckets = self.rtt_histogram_buckets();
        let bucket_labels = ["0.01", "0.05", "0.1", "0.5", "1.0", "+Inf"];
        let mut cumulative = 0u64;
        for (i, &count) in buckets.iter().enumerate() {
            cumulative += count;
            output.push_str(&format!(
                "veryslip_rtt_seconds_bucket{{le=\"{}\"}} {}\n",
                bucket_labels[i], cumulative
            ));
        }
        output.push_str(&format!("veryslip_rtt_seconds_count {}\n", cumulative));
        output.push('\n');
        
        // Per-domain queries
        output.push_str("# HELP veryslip_domain_queries_total Total queries per domain\n");
        output.push_str("# TYPE veryslip_domain_queries_total counter\n");
        for (domain, count) in self.domain_stats_snapshot() {
            output.push_str(&format!(
                "veryslip_domain_queries_total{{domain=\"{}\"}} {}\n",
                domain, count
            ));
        }
        output.push('\n');
        
        output
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        assert_eq!(collector.bytes_sent(), 0);
        assert_eq!(collector.bytes_received(), 0);
        assert_eq!(collector.queries_processed(), 0);
    }
    
    #[test]
    fn test_record_bytes() {
        let collector = MetricsCollector::new();
        collector.record_bytes_sent(100);
        collector.record_bytes_received(200);
        
        assert_eq!(collector.bytes_sent(), 100);
        assert_eq!(collector.bytes_received(), 200);
    }
    
    #[test]
    fn test_record_query() {
        let collector = MetricsCollector::new();
        collector.record_query("example.com", false);
        collector.record_query("example.com", true);
        collector.record_query("test.com", false);
        
        assert_eq!(collector.queries_processed(), 3);
        assert_eq!(collector.queries_batched(), 1);
        
        let domain_stats = collector.domain_stats_snapshot();
        assert_eq!(domain_stats.get("example.com"), Some(&2));
        assert_eq!(domain_stats.get("test.com"), Some(&1));
    }
    
    #[test]
    fn test_compression_ratio() {
        let collector = MetricsCollector::new();
        
        // No compression yet
        assert_eq!(collector.compression_ratio(), 0.0);
        
        // 50% compression (1000 -> 500)
        collector.record_compression(1000, 500);
        assert!((collector.compression_ratio() - 0.5).abs() < 0.001);
        
        // Add more (2000 total in, 1000 total out = 50% overall)
        collector.record_compression(1000, 500);
        assert!((collector.compression_ratio() - 0.5).abs() < 0.001);
    }
    
    #[test]
    fn test_rtt_histogram() {
        let collector = MetricsCollector::new();
        
        collector.record_rtt(5);    // Bucket 0 (0-10ms)
        collector.record_rtt(25);   // Bucket 1 (10-50ms)
        collector.record_rtt(75);   // Bucket 2 (50-100ms)
        collector.record_rtt(250);  // Bucket 3 (100-500ms)
        collector.record_rtt(750);  // Bucket 4 (500-1000ms)
        collector.record_rtt(1500); // Bucket 5 (1000ms+)
        
        let buckets = collector.rtt_histogram_buckets();
        assert_eq!(buckets, [1, 1, 1, 1, 1, 1]);
    }
    
    #[test]
    fn test_active_connections_and_streams() {
        let collector = MetricsCollector::new();
        
        collector.set_active_connections(10);
        collector.set_active_streams(50);
        
        assert_eq!(collector.active_connections(), 10);
        assert_eq!(collector.active_streams(), 50);
    }
    
    #[test]
    fn test_prometheus_export() {
        let collector = MetricsCollector::new();
        
        collector.record_bytes_sent(1000);
        collector.record_bytes_received(2000);
        collector.record_query("example.com", false);
        collector.set_active_connections(5);
        
        let output = collector.export_prometheus();
        
        assert!(output.contains("veryslip_bytes_sent_total 1000"));
        assert!(output.contains("veryslip_bytes_received_total 2000"));
        assert!(output.contains("veryslip_queries_total 1"));
        assert!(output.contains("veryslip_active_connections 5"));
        assert!(output.contains("domain=\"example.com\""));
    }
}
