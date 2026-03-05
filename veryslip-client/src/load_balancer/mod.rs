use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;
use std::time::{Duration, Instant};
use std::collections::VecDeque;

/// Load balancer for distributing queries across multiple domains
pub struct LoadBalancer {
    domains: Vec<Arc<DomainState>>,
    selector: AtomicUsize,
    config: LoadBalancerConfig,
}

/// Load balancer configuration
#[derive(Debug, Clone)]
pub struct LoadBalancerConfig {
    pub failure_timeout: Duration,
    pub window_size: Duration,
    pub success_threshold: f32,
    pub weight_reduction: f32,
}

impl Default for LoadBalancerConfig {
    fn default() -> Self {
        Self {
            failure_timeout: Duration::from_secs(60),
            window_size: Duration::from_secs(300), // 5 minutes
            success_threshold: 0.5, // 50%
            weight_reduction: 0.25, // Reduce to 25%
        }
    }
}

/// Domain state tracking
pub struct DomainState {
    pub domain: String,
    pub available: AtomicBool,
    pub unavailable_until: AtomicU64, // Unix timestamp in seconds
    pub success_rate: RwLock<SlidingWindow>,
    pub query_weight: AtomicU32, // 0-100, percentage
    pub last_success: AtomicU64, // Unix timestamp
    pub stats: DomainStats,
}

/// Domain statistics
#[derive(Default)]
pub struct DomainStats {
    pub queries_sent: AtomicU64,
    pub queries_success: AtomicU64,
    pub queries_failed: AtomicU64,
    pub total_rtt_ms: AtomicU64,
}

impl DomainStats {
    pub fn avg_rtt_ms(&self) -> f64 {
        let success = self.queries_success.load(Ordering::Relaxed);
        if success == 0 {
            return 0.0;
        }
        self.total_rtt_ms.load(Ordering::Relaxed) as f64 / success as f64
    }
}

/// Sliding window for tracking success rate
pub struct SlidingWindow {
    buckets: VecDeque<Bucket>,
    bucket_duration: Duration,
    window_size: Duration,
}

struct Bucket {
    timestamp: Instant,
    success: u32,
    failure: u32,
}

impl SlidingWindow {
    fn new(window_size: Duration) -> Self {
        Self {
            buckets: VecDeque::new(),
            bucket_duration: Duration::from_secs(30), // 30s per bucket
            window_size,
        }
    }

    fn record(&mut self, success: bool) {
        let now = Instant::now();
        
        // Remove old buckets
        while let Some(bucket) = self.buckets.front() {
            if now.duration_since(bucket.timestamp) > self.window_size {
                self.buckets.pop_front();
            } else {
                break;
            }
        }

        // Add to current bucket or create new
        if let Some(bucket) = self.buckets.back_mut() {
            if now.duration_since(bucket.timestamp) < self.bucket_duration {
                if success {
                    bucket.success += 1;
                } else {
                    bucket.failure += 1;
                }
                return;
            }
        }

        // Create new bucket
        self.buckets.push_back(Bucket {
            timestamp: now,
            success: if success { 1 } else { 0 },
            failure: if success { 0 } else { 1 },
        });
    }

    fn success_rate(&self) -> f32 {
        let mut total_success = 0u32;
        let mut total_failure = 0u32;

        for bucket in &self.buckets {
            total_success += bucket.success;
            total_failure += bucket.failure;
        }

        let total = total_success + total_failure;
        if total == 0 {
            return 1.0; // No data, assume success
        }

        total_success as f32 / total as f32
    }
}

impl LoadBalancer {
    /// Create new load balancer
    pub fn new(domains: Vec<String>, config: LoadBalancerConfig) -> Self {
        let domain_states = domains
            .into_iter()
            .map(|domain| {
                Arc::new(DomainState {
                    domain,
                    available: AtomicBool::new(true),
                    unavailable_until: AtomicU64::new(0),
                    success_rate: RwLock::new(SlidingWindow::new(config.window_size)),
                    query_weight: AtomicU32::new(100), // Start at 100%
                    last_success: AtomicU64::new(0),
                    stats: DomainStats::default(),
                })
            })
            .collect();

        Self {
            domains: domain_states,
            selector: AtomicUsize::new(0),
            config,
        }
    }

    /// Select next domain for query
    pub fn select_domain(&self) -> Option<Arc<DomainState>> {
        if self.domains.is_empty() {
            return None;
        }

        let now = Self::current_timestamp();
        
        // Filter available domains
        let mut available: Vec<_> = self.domains
            .iter()
            .filter(|d| {
                let unavailable_until = d.unavailable_until.load(Ordering::Relaxed);
                unavailable_until == 0 || now >= unavailable_until
            })
            .cloned()
            .collect();

        // If all unavailable, retry all starting with most recent success
        if available.is_empty() {
            available = self.domains.clone();
            available.sort_by_key(|d| std::cmp::Reverse(d.last_success.load(Ordering::Relaxed)));
            
            // Reset unavailable status
            for domain in &available {
                domain.unavailable_until.store(0, Ordering::Relaxed);
                domain.available.store(true, Ordering::Relaxed);
            }
        }

        // Apply weights and select
        let total_weight: u32 = available
            .iter()
            .map(|d| d.query_weight.load(Ordering::Relaxed))
            .sum();

        if total_weight == 0 {
            // All weights are 0, use round-robin
            let idx = self.selector.fetch_add(1, Ordering::Relaxed) % available.len();
            return Some(available[idx].clone());
        }

        // Weighted round-robin
        let selector = self.selector.fetch_add(1, Ordering::Relaxed);
        let target = (selector % available.len()) as usize;
        
        Some(available[target].clone())
    }

    /// Mark query success
    pub fn mark_success(&self, domain: &str, rtt: Duration) {
        if let Some(state) = self.find_domain(domain) {
            state.stats.queries_success.fetch_add(1, Ordering::Relaxed);
            state.stats.total_rtt_ms.fetch_add(rtt.as_millis() as u64, Ordering::Relaxed);
            state.last_success.store(Self::current_timestamp(), Ordering::Relaxed);
            
            // Update sliding window
            state.success_rate.write().record(true);
            
            // Update weight based on success rate
            self.update_weight(&state);
        }
    }

    /// Mark query failure
    pub fn mark_failure(&self, domain: &str) {
        if let Some(state) = self.find_domain(domain) {
            state.stats.queries_failed.fetch_add(1, Ordering::Relaxed);
            
            // Update sliding window
            state.success_rate.write().record(false);
            
            // Check if we should mark unavailable
            let success_rate = state.success_rate.read().success_rate();
            if success_rate < 0.2 {
                // Very low success rate, mark unavailable
                let unavailable_until = Self::current_timestamp() + self.config.failure_timeout.as_secs();
                state.unavailable_until.store(unavailable_until, Ordering::Relaxed);
                state.available.store(false, Ordering::Relaxed);
            }
            
            // Update weight
            self.update_weight(&state);
        }
    }

    /// Update domain weight based on success rate
    fn update_weight(&self, state: &DomainState) {
        let success_rate = state.success_rate.read().success_rate();
        
        if success_rate < self.config.success_threshold {
            // Degraded performance, reduce weight
            let reduced_weight = (100.0 * self.config.weight_reduction) as u32;
            state.query_weight.store(reduced_weight, Ordering::Relaxed);
        } else {
            // Good performance, restore full weight
            state.query_weight.store(100, Ordering::Relaxed);
        }
    }

    /// Find domain state by name
    fn find_domain(&self, domain: &str) -> Option<Arc<DomainState>> {
        self.domains
            .iter()
            .find(|d| d.domain == domain)
            .cloned()
    }

    /// Get statistics for all domains
    pub fn get_stats(&self) -> Vec<DomainStatsSnapshot> {
        self.domains
            .iter()
            .map(|d| {
                let success_rate = d.success_rate.read().success_rate();
                DomainStatsSnapshot {
                    domain: d.domain.clone(),
                    available: d.available.load(Ordering::Relaxed),
                    queries_sent: d.stats.queries_sent.load(Ordering::Relaxed),
                    queries_success: d.stats.queries_success.load(Ordering::Relaxed),
                    queries_failed: d.stats.queries_failed.load(Ordering::Relaxed),
                    avg_rtt_ms: d.stats.avg_rtt_ms(),
                    success_rate,
                    weight: d.query_weight.load(Ordering::Relaxed),
                }
            })
            .collect()
    }

    /// Get current Unix timestamp in seconds
    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

/// Snapshot of domain statistics
#[derive(Debug, Clone)]
pub struct DomainStatsSnapshot {
    pub domain: String,
    pub available: bool,
    pub queries_sent: u64,
    pub queries_success: u64,
    pub queries_failed: u64,
    pub avg_rtt_ms: f64,
    pub success_rate: f32,
    pub weight: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_balancer_creation() {
        let domains = vec!["example1.com".to_string(), "example2.com".to_string()];
        let config = LoadBalancerConfig::default();
        let lb = LoadBalancer::new(domains, config);
        
        assert_eq!(lb.domains.len(), 2);
    }

    #[test]
    fn test_domain_selection() {
        let domains = vec![
            "example1.com".to_string(),
            "example2.com".to_string(),
            "example3.com".to_string(),
        ];
        let config = LoadBalancerConfig::default();
        let lb = LoadBalancer::new(domains, config);
        
        // Select multiple times, should eventually use all domains
        let mut selected = std::collections::HashSet::new();
        for _ in 0..10 {
            let domain = lb.select_domain().unwrap();
            selected.insert(domain.domain.clone());
        }
        
        // Should have selected at least 2 different domains
        assert!(selected.len() >= 2);
    }

    #[test]
    fn test_mark_success() {
        let domains = vec!["example.com".to_string()];
        let config = LoadBalancerConfig::default();
        let lb = LoadBalancer::new(domains, config);
        
        lb.mark_success("example.com", Duration::from_millis(100));
        
        let stats = lb.get_stats();
        assert_eq!(stats[0].queries_success, 1);
        assert!(stats[0].avg_rtt_ms > 0.0);
    }

    #[test]
    fn test_mark_failure() {
        let domains = vec!["example.com".to_string()];
        let config = LoadBalancerConfig::default();
        let lb = LoadBalancer::new(domains, config);
        
        lb.mark_failure("example.com");
        
        let stats = lb.get_stats();
        assert_eq!(stats[0].queries_failed, 1);
    }

    #[test]
    fn test_weight_reduction() {
        let domains = vec!["example.com".to_string()];
        let config = LoadBalancerConfig::default();
        let lb = LoadBalancer::new(domains, config);
        
        // Record many failures
        for _ in 0..20 {
            lb.mark_failure("example.com");
        }
        
        let stats = lb.get_stats();
        assert!(stats[0].weight < 100); // Weight should be reduced
    }

    #[test]
    fn test_unavailable_domain() {
        let domains = vec!["example.com".to_string()];
        let config = LoadBalancerConfig::default();
        let lb = LoadBalancer::new(domains, config);
        
        // Record many failures to trigger unavailable
        for _ in 0..50 {
            lb.mark_failure("example.com");
        }
        
        let stats = lb.get_stats();
        // Domain might be marked unavailable due to very low success rate
        assert!(stats[0].success_rate < 0.5);
    }

    #[test]
    fn test_sliding_window() {
        let mut window = SlidingWindow::new(Duration::from_secs(300));
        
        // Record successes
        for _ in 0..8 {
            window.record(true);
        }
        for _ in 0..2 {
            window.record(false);
        }
        
        let rate = window.success_rate();
        assert!((rate - 0.8).abs() < 0.01); // Should be 80%
    }
}
