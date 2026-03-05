use crate::{Result};
use std::time::Duration;
use tokio::time::sleep;

/// Exponential backoff configuration
#[derive(Debug, Clone)]
pub struct BackoffConfig {
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            multiplier: 2.0,
        }
    }
}

/// Exponential backoff state
pub struct Backoff {
    config: BackoffConfig,
    current_delay: Duration,
    attempt: u32,
}

impl Backoff {
    pub fn new(config: BackoffConfig) -> Self {
        Self {
            current_delay: config.initial_delay,
            config,
            attempt: 0,
        }
    }

    /// Wait for the current delay and increment for next attempt
    pub async fn wait(&mut self) {
        sleep(self.current_delay).await;
        self.attempt += 1;
        
        // Calculate next delay with exponential backoff
        let next_delay_secs = self.current_delay.as_secs_f64() * self.config.multiplier;
        self.current_delay = Duration::from_secs_f64(next_delay_secs.min(self.config.max_delay.as_secs_f64()));
    }

    /// Reset backoff to initial state
    pub fn reset(&mut self) {
        self.current_delay = self.config.initial_delay;
        self.attempt = 0;
    }

    /// Get current attempt number
    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

/// Retry a fallible async operation with exponential backoff
pub async fn retry_with_backoff<F, Fut, T>(
    mut operation: F,
    config: BackoffConfig,
    max_attempts: Option<u32>,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut backoff = Backoff::new(config);
    
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if let Some(max) = max_attempts {
                    if backoff.attempt() >= max {
                        return Err(e);
                    }
                }
                
                tracing::warn!(
                    "Operation failed (attempt {}): {}. Retrying after {:?}",
                    backoff.attempt() + 1,
                    e,
                    backoff.current_delay
                );
                
                backoff.wait().await;
            }
        }
    }
}

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,  // Normal operation
    Open,    // Failing, reject requests
    HalfOpen, // Testing if recovered
}

/// Circuit breaker for preventing cascading failures
pub struct CircuitBreaker {
    state: std::sync::Mutex<CircuitState>,
    failure_threshold: u32,
    success_threshold: u32,
    timeout: Duration,
    failure_count: std::sync::atomic::AtomicU32,
    success_count: std::sync::atomic::AtomicU32,
    last_failure_time: std::sync::Mutex<Option<std::time::Instant>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, success_threshold: u32, timeout: Duration) -> Self {
        Self {
            state: std::sync::Mutex::new(CircuitState::Closed),
            failure_threshold,
            success_threshold,
            timeout,
            failure_count: std::sync::atomic::AtomicU32::new(0),
            success_count: std::sync::atomic::AtomicU32::new(0),
            last_failure_time: std::sync::Mutex::new(None),
        }
    }

    /// Check if request should be allowed
    pub fn allow_request(&self) -> bool {
        let state = *self.state.lock().unwrap();
        
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has elapsed
                if let Some(last_failure) = *self.last_failure_time.lock().unwrap() {
                    if last_failure.elapsed() >= self.timeout {
                        // Transition to half-open
                        *self.state.lock().unwrap() = CircuitState::HalfOpen;
                        self.success_count.store(0, std::sync::atomic::Ordering::Relaxed);
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record successful operation
    pub fn record_success(&self) {
        let state = *self.state.lock().unwrap();
        
        match state {
            CircuitState::Closed => {
                self.failure_count.store(0, std::sync::atomic::Ordering::Relaxed);
            }
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if successes >= self.success_threshold {
                    // Transition back to closed
                    *self.state.lock().unwrap() = CircuitState::Closed;
                    self.failure_count.store(0, std::sync::atomic::Ordering::Relaxed);
                    tracing::info!("Circuit breaker closed after {} successful attempts", successes);
                }
            }
            CircuitState::Open => {}
        }
    }

    /// Record failed operation
    pub fn record_failure(&self) {
        let failures = self.failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        *self.last_failure_time.lock().unwrap() = Some(std::time::Instant::now());
        
        let mut state = self.state.lock().unwrap();
        
        match *state {
            CircuitState::Closed => {
                if failures >= self.failure_threshold {
                    // Transition to open
                    *state = CircuitState::Open;
                    tracing::warn!("Circuit breaker opened after {} failures", failures);
                }
            }
            CircuitState::HalfOpen => {
                // Transition back to open
                *state = CircuitState::Open;
                self.success_count.store(0, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!("Circuit breaker reopened after failure in half-open state");
            }
            CircuitState::Open => {}
        }
    }

    /// Get current state
    pub fn state(&self) -> CircuitState {
        *self.state.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VerySlipError;

    #[tokio::test]
    async fn test_backoff_progression() {
        let config = BackoffConfig {
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            multiplier: 2.0,
        };
        
        let mut backoff = Backoff::new(config);
        
        assert_eq!(backoff.attempt(), 0);
        assert_eq!(backoff.current_delay, Duration::from_millis(10));
        
        backoff.wait().await;
        assert_eq!(backoff.attempt(), 1);
        assert_eq!(backoff.current_delay, Duration::from_millis(20));
        
        backoff.wait().await;
        assert_eq!(backoff.attempt(), 2);
        assert_eq!(backoff.current_delay, Duration::from_millis(40));
        
        backoff.wait().await;
        assert_eq!(backoff.attempt(), 3);
        assert_eq!(backoff.current_delay, Duration::from_millis(80));
        
        backoff.wait().await;
        assert_eq!(backoff.attempt(), 4);
        // Should cap at max_delay
        assert_eq!(backoff.current_delay, Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_backoff_reset() {
        let config = BackoffConfig::default();
        let mut backoff = Backoff::new(config.clone());
        
        backoff.wait().await;
        backoff.wait().await;
        assert_eq!(backoff.attempt(), 2);
        
        backoff.reset();
        assert_eq!(backoff.attempt(), 0);
        assert_eq!(backoff.current_delay, config.initial_delay);
    }

    #[tokio::test]
    async fn test_retry_success() {
        let config = BackoffConfig {
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            multiplier: 2.0,
        };
        
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let attempts_clone = attempts.clone();
        
        let result = retry_with_backoff(
            move || {
                let attempts = attempts_clone.clone();
                async move {
                    let count = attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    if count < 3 {
                        Err(VerySlipError::Network("test error".to_string()))
                    } else {
                        Ok(42)
                    }
                }
            },
            config,
            Some(5),
        ).await;
        
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn test_retry_max_attempts() {
        let config = BackoffConfig {
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            multiplier: 2.0,
        };
        
        let result = retry_with_backoff(
            || async {
                Err::<(), _>(VerySlipError::Network("test error".to_string()))
            },
            config,
            Some(3),
        ).await;
        
        assert!(result.is_err());
    }

    #[test]
    fn test_circuit_breaker_closed_to_open() {
        let cb = CircuitBreaker::new(3, 2, Duration::from_secs(60));
        
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
        
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
        
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_circuit_breaker_half_open_to_closed() {
        let cb = CircuitBreaker::new(3, 2, Duration::from_millis(10));
        
        // Open the circuit
        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        
        // Wait for timeout
        std::thread::sleep(Duration::from_millis(20));
        
        // Should transition to half-open
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        
        // Record successes
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_half_open_to_open() {
        let cb = CircuitBreaker::new(3, 2, Duration::from_millis(10));
        
        // Open the circuit
        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        
        // Wait for timeout
        std::thread::sleep(Duration::from_millis(20));
        
        // Transition to half-open
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        
        // Failure in half-open should reopen
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }
}
