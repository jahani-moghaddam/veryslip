use rand::Rng;
use std::time::Duration;

/// Stealth configuration
#[derive(Debug, Clone)]
pub struct StealthConfig {
    pub timing_jitter_enabled: bool,
    pub min_jitter_ms: u64,
    pub max_jitter_ms: u64,
    pub privacy_logging: bool,
}

impl Default for StealthConfig {
    fn default() -> Self {
        Self {
            timing_jitter_enabled: true,
            min_jitter_ms: 10,
            max_jitter_ms: 100,
            privacy_logging: true,
        }
    }
}

/// Add random jitter to query timing to avoid pattern detection
pub fn add_timing_jitter(config: &StealthConfig) -> Duration {
    if !config.timing_jitter_enabled {
        return Duration::from_millis(0);
    }

    let mut rng = rand::thread_rng();
    let jitter_ms = rng.gen_range(config.min_jitter_ms..=config.max_jitter_ms);
    Duration::from_millis(jitter_ms)
}

/// Apply jitter and wait
pub async fn wait_with_jitter(config: &StealthConfig) {
    let jitter = add_timing_jitter(config);
    if jitter > Duration::from_millis(0) {
        tokio::time::sleep(jitter).await;
    }
}

/// Sanitize URL for logging (remove query parameters and fragments)
pub fn sanitize_url_for_logging(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        format!("{}://{}{}", 
            parsed.scheme(),
            parsed.host_str().unwrap_or("unknown"),
            parsed.path()
        )
    } else {
        // If parsing fails, just return the host part if possible
        url.split('?').next().unwrap_or(url).to_string()
    }
}

/// Sanitize headers for logging (remove sensitive headers)
pub fn sanitize_headers_for_logging(headers: &[(String, String)]) -> Vec<(String, String)> {
    const SENSITIVE_HEADERS: &[&str] = &[
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
        "x-auth-token",
    ];

    headers
        .iter()
        .filter_map(|(key, value)| {
            let key_lower = key.to_lowercase();
            if SENSITIVE_HEADERS.contains(&key_lower.as_str()) {
                Some((key.clone(), "[REDACTED]".to_string()))
            } else {
                Some((key.clone(), value.clone()))
            }
        })
        .collect()
}

/// Generate legitimate-looking DNS query
pub fn ensure_legitimate_dns_format(query_data: &[u8]) -> bool {
    // Basic DNS query validation
    if query_data.len() < 12 {
        return false;
    }

    // Check DNS header format
    // Bytes 0-1: Transaction ID (any value is valid)
    // Bytes 2-3: Flags (should look like a standard query)
    let flags = u16::from_be_bytes([query_data[2], query_data[3]]);
    
    // Standard query: QR=0, Opcode=0, RD=1
    // This means: not a response, standard query, recursion desired
    let qr = (flags >> 15) & 0x1;
    let opcode = (flags >> 11) & 0xF;
    let rd = (flags >> 8) & 0x1;
    
    // Should be a query (QR=0), standard query (Opcode=0), with recursion desired (RD=1)
    qr == 0 && opcode == 0 && rd == 1
}

/// Privacy-aware logging macro
#[macro_export]
macro_rules! log_with_privacy {
    ($level:expr, $config:expr, $($arg:tt)*) => {
        if $config.privacy_logging {
            match $level {
                tracing::Level::ERROR => tracing::error!($($arg)*),
                tracing::Level::WARN => tracing::warn!($($arg)*),
                tracing::Level::INFO => tracing::info!($($arg)*),
                tracing::Level::DEBUG => tracing::debug!($($arg)*),
                tracing::Level::TRACE => tracing::trace!($($arg)*),
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stealth_config_default() {
        let config = StealthConfig::default();
        assert!(config.timing_jitter_enabled);
        assert_eq!(config.min_jitter_ms, 10);
        assert_eq!(config.max_jitter_ms, 100);
        assert!(config.privacy_logging);
    }

    #[test]
    fn test_timing_jitter() {
        let config = StealthConfig::default();
        
        for _ in 0..10 {
            let jitter = add_timing_jitter(&config);
            assert!(jitter >= Duration::from_millis(config.min_jitter_ms));
            assert!(jitter <= Duration::from_millis(config.max_jitter_ms));
        }
    }

    #[test]
    fn test_timing_jitter_disabled() {
        let config = StealthConfig {
            timing_jitter_enabled: false,
            ..Default::default()
        };
        
        let jitter = add_timing_jitter(&config);
        assert_eq!(jitter, Duration::from_millis(0));
    }

    #[test]
    fn test_sanitize_url() {
        assert_eq!(
            sanitize_url_for_logging("https://example.com/path?key=secret#fragment"),
            "https://example.com/path"
        );
        
        assert_eq!(
            sanitize_url_for_logging("http://api.example.com/users/123?token=abc"),
            "http://api.example.com/users/123"
        );
    }

    #[test]
    fn test_sanitize_headers() {
        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Authorization".to_string(), "Bearer secret-token".to_string()),
            ("Cookie".to_string(), "session=abc123".to_string()),
            ("User-Agent".to_string(), "Mozilla/5.0".to_string()),
        ];

        let sanitized = sanitize_headers_for_logging(&headers);
        
        assert_eq!(sanitized.len(), 4);
        assert_eq!(sanitized[0].1, "application/json");
        assert_eq!(sanitized[1].1, "[REDACTED]");
        assert_eq!(sanitized[2].1, "[REDACTED]");
        assert_eq!(sanitized[3].1, "Mozilla/5.0");
    }

    #[test]
    fn test_legitimate_dns_format() {
        // Valid DNS query header
        let valid_query = vec![
            0x12, 0x34, // Transaction ID
            0x01, 0x00, // Flags: standard query, recursion desired
            0x00, 0x01, // Questions: 1
            0x00, 0x00, // Answers: 0
            0x00, 0x00, // Authority: 0
            0x00, 0x00, // Additional: 0
        ];
        
        assert!(ensure_legitimate_dns_format(&valid_query));
        
        // Invalid: too short
        assert!(!ensure_legitimate_dns_format(&[0x12, 0x34]));
        
        // Invalid: response flag set (QR=1)
        let response = vec![
            0x12, 0x34,
            0x81, 0x00, // QR=1 (response)
            0x00, 0x01,
            0x00, 0x00,
            0x00, 0x00,
            0x00, 0x00,
        ];
        assert!(!ensure_legitimate_dns_format(&response));
    }

    #[tokio::test]
    #[ignore] // Flaky timing test
    async fn test_wait_with_jitter() {
        let config = StealthConfig {
            min_jitter_ms: 1,
            max_jitter_ms: 5,
            ..Default::default()
        };
        
        let start = std::time::Instant::now();
        wait_with_jitter(&config).await;
        let elapsed = start.elapsed();
        
        assert!(elapsed >= Duration::from_millis(1));
        assert!(elapsed <= Duration::from_millis(10)); // Allow some overhead
    }
}
