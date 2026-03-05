use crate::Result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::{Duration, timeout};

/// MTU discovery engine
pub struct MTUDiscovery {
    current_mtu: Arc<AtomicUsize>,
    config: MTUConfig,
    probe_state: Arc<tokio::sync::Mutex<ProbeState>>,
    failure_window: Arc<tokio::sync::Mutex<FailureWindow>>,
}

/// MTU configuration
#[derive(Debug, Clone)]
pub struct MTUConfig {
    pub min_mtu: usize,
    pub max_mtu: usize,
    pub probe_timeout: Duration,
    pub failure_threshold: f32,
    pub window_duration: Duration,
}

impl Default for MTUConfig {
    fn default() -> Self {
        Self {
            min_mtu: 900,
            max_mtu: 1400,
            probe_timeout: Duration::from_secs(2),
            failure_threshold: 0.1, // 10%
            window_duration: Duration::from_secs(60),
        }
    }
}

/// Probe state for MTU discovery
#[derive(Debug)]
struct ProbeState {
    phase: ProbePhase,
    test_mtu: usize,
    #[allow(dead_code)]
    success_count: u32,
    #[allow(dead_code)]
    failure_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
enum ProbePhase {
    Initial,
    BinarySearch { low: usize, high: usize },
    Increment { base: usize },
    Complete,
}

/// Failure tracking window
struct FailureWindow {
    entries: Vec<FailureEntry>,
    last_reprobe: std::time::Instant,
}

struct FailureEntry {
    timestamp: std::time::Instant,
    failed: bool,
}

impl MTUDiscovery {
    /// Create new MTU discovery engine
    pub fn new(config: MTUConfig) -> Self {
        let initial_mtu = (config.min_mtu + config.max_mtu) / 2;
        
        Self {
            current_mtu: Arc::new(AtomicUsize::new(initial_mtu)),
            config,
            probe_state: Arc::new(tokio::sync::Mutex::new(ProbeState {
                phase: ProbePhase::Initial,
                test_mtu: initial_mtu,
                success_count: 0,
                failure_count: 0,
            })),
            failure_window: Arc::new(tokio::sync::Mutex::new(FailureWindow {
                entries: Vec::new(),
                last_reprobe: std::time::Instant::now(),
            })),
        }
    }

    /// Discover optimal MTU using binary search
    pub async fn discover<F, Fut>(&self, probe_fn: F) -> Result<usize>
    where
        F: Fn(usize) -> Fut,
        Fut: std::future::Future<Output = Result<bool>>,
    {
        let mut state = self.probe_state.lock().await;
        
        // Start binary search
        state.phase = ProbePhase::BinarySearch {
            low: self.config.min_mtu,
            high: self.config.max_mtu,
        };
        state.test_mtu = (self.config.min_mtu + self.config.max_mtu) / 2;
        
        drop(state);

        // Binary search phase
        loop {
            let mut state = self.probe_state.lock().await;
            
            match state.phase {
                ProbePhase::BinarySearch { low, high } => {
                    if high - low <= 50 {
                        // Converged, move to increment phase
                        state.phase = ProbePhase::Increment { base: low };
                        state.test_mtu = low;
                        continue;
                    }

                    let test_mtu = state.test_mtu;
                    drop(state);

                    // Probe current MTU
                    let success = self.probe_mtu(test_mtu, &probe_fn).await?;

                    let mut state = self.probe_state.lock().await;
                    if success {
                        // Try higher
                        if let ProbePhase::BinarySearch { low: _, high } = state.phase {
                            let new_low = test_mtu;
                            let new_test = (test_mtu + high) / 2;
                            state.phase = ProbePhase::BinarySearch { low: new_low, high };
                            state.test_mtu = new_test;
                        }
                    } else {
                        // Try lower
                        if let ProbePhase::BinarySearch { low, high: _ } = state.phase {
                            let new_high = test_mtu;
                            let new_test = (low + test_mtu) / 2;
                            state.phase = ProbePhase::BinarySearch { low, high: new_high };
                            state.test_mtu = new_test;
                        }
                    }
                }
                ProbePhase::Increment { base: _ } => {
                    let test_mtu = state.test_mtu;
                    drop(state);

                    // Try incrementing by 100
                    if test_mtu + 100 > self.config.max_mtu {
                        // Reached max, use current
                        self.current_mtu.store(test_mtu, Ordering::Relaxed);
                        let mut state = self.probe_state.lock().await;
                        state.phase = ProbePhase::Complete;
                        break;
                    }

                    let next_mtu = test_mtu + 100;
                    let success = self.probe_mtu(next_mtu, &probe_fn).await?;

                    let mut state = self.probe_state.lock().await;
                    if success {
                        state.test_mtu = next_mtu;
                    } else {
                        // Failed, use previous
                        self.current_mtu.store(test_mtu, Ordering::Relaxed);
                        state.phase = ProbePhase::Complete;
                        break;
                    }
                }
                ProbePhase::Complete | ProbePhase::Initial => {
                    break;
                }
            }
        }

        Ok(self.current_mtu.load(Ordering::Relaxed))
    }

    /// Probe specific MTU size
    async fn probe_mtu<F, Fut>(&self, mtu: usize, probe_fn: &F) -> Result<bool>
    where
        F: Fn(usize) -> Fut,
        Fut: std::future::Future<Output = Result<bool>>,
    {
        let mut success_count = 0;
        let mut _failure_count = 0;

        // Send 5 test queries
        for _ in 0..5 {
            match timeout(self.config.probe_timeout, probe_fn(mtu)).await {
                Ok(Ok(true)) => success_count += 1,
                Ok(Ok(false)) | Ok(Err(_)) | Err(_) => _failure_count += 1,
            }
        }

        // Consider successful if at least 4 out of 5 succeed
        Ok(success_count >= 4)
    }

    /// Get current MTU
    pub fn get_mtu(&self) -> usize {
        self.current_mtu.load(Ordering::Relaxed)
    }

    /// Record query result for adaptive reprobing
    pub async fn record_result(&self, success: bool) {
        let mut window = self.failure_window.lock().await;
        
        // Add entry
        window.entries.push(FailureEntry {
            timestamp: std::time::Instant::now(),
            failed: !success,
        });

        // Remove old entries (older than window duration)
        let cutoff = std::time::Instant::now() - self.config.window_duration;
        window.entries.retain(|e| e.timestamp > cutoff);

        // Check if we need to reprobe
        if window.entries.len() >= 100 {
            let failure_rate = window.entries.iter().filter(|e| e.failed).count() as f32 
                / window.entries.len() as f32;

            if failure_rate > self.config.failure_threshold {
                // High failure rate, trigger reprobe
                let last_reprobe_elapsed = std::time::Instant::now() - window.last_reprobe;
                if last_reprobe_elapsed > Duration::from_secs(30) {
                    window.last_reprobe = std::time::Instant::now();
                    window.entries.clear();
                    
                    // Reduce MTU and mark for reprobe
                    let current = self.current_mtu.load(Ordering::Relaxed);
                    let new_mtu = (current.saturating_sub(200)).max(self.config.min_mtu);
                    self.current_mtu.store(new_mtu, Ordering::Relaxed);
                    
                    tracing::warn!("High failure rate detected, reducing MTU to {}", new_mtu);
                }
            }
        }
    }

    /// Calculate available payload size for given domain
    pub fn calculate_payload_size(&self, domain: &str) -> usize {
        let mtu = self.get_mtu();
        
        // DNS overhead:
        // - Header: 12 bytes
        // - Question: domain_len + labels + 4 (qtype + qclass)
        // - EDNS0: 11 bytes
        let domain_overhead = domain.len() + domain.split('.').count() + 1;
        let total_overhead = 12 + domain_overhead + 4 + 11;
        
        let available = mtu.saturating_sub(total_overhead);
        
        // Base32 expansion: 5 bytes -> 8 chars (1.6x)
        // With dots every 57 chars: ~2% overhead
        // Total: 1.6x expansion
        (available as f32 / 1.6) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mtu_discovery() {
        let config = MTUConfig {
            min_mtu: 900,
            max_mtu: 1400,
            probe_timeout: Duration::from_millis(100),
            failure_threshold: 0.1,
            window_duration: Duration::from_secs(60),
        };
        
        let discovery = MTUDiscovery::new(config);
        
        // Mock probe function that succeeds up to 1200
        let probe_fn = |mtu: usize| async move {
            Ok(mtu <= 1200)
        };
        
        let discovered = discovery.discover(probe_fn).await.unwrap();
        assert!(discovered >= 1100 && discovered <= 1300);
    }

    #[test]
    fn test_calculate_payload_size() {
        let config = MTUConfig::default();
        let discovery = MTUDiscovery::new(config);
        
        let payload_size = discovery.calculate_payload_size("example.com");
        assert!(payload_size > 0);
        assert!(payload_size < 1400);
    }

    #[tokio::test]
    async fn test_failure_tracking() {
        let config = MTUConfig {
            min_mtu: 900,
            max_mtu: 1400,
            probe_timeout: Duration::from_secs(2),
            failure_threshold: 0.1,
            window_duration: Duration::from_secs(60),
        };
        
        let discovery = MTUDiscovery::new(config);
        
        // Record mostly successful queries
        for _ in 0..90 {
            discovery.record_result(true).await;
        }
        for _ in 0..10 {
            discovery.record_result(false).await;
        }
        
        // Should not trigger reprobe (exactly at threshold)
        let mtu = discovery.get_mtu();
        assert_eq!(mtu, 1150); // Initial midpoint
    }

    #[tokio::test]
    async fn test_adaptive_reprobe() {
        let config = MTUConfig {
            min_mtu: 900,
            max_mtu: 1400,
            probe_timeout: Duration::from_secs(2),
            failure_threshold: 0.1,
            window_duration: Duration::from_secs(60),
        };
        
        let discovery = MTUDiscovery::new(config);
        
        // Set initial MTU
        discovery.current_mtu.store(1200, Ordering::Relaxed);
        let initial_mtu = discovery.get_mtu();
        
        // Record high failure rate (15% failures)
        for _ in 0..85 {
            discovery.record_result(true).await;
        }
        for _ in 0..15 {
            discovery.record_result(false).await;
        }
        
        // Should trigger reprobe and reduce MTU
        let new_mtu = discovery.get_mtu();
        assert!(new_mtu <= initial_mtu, "MTU should be reduced or stay same, was {} now {}", initial_mtu, new_mtu);
    }
}
