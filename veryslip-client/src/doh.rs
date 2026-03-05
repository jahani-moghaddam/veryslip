use crate::{Result, VerySlipError};
use crate::dns::{DnsQuery, DnsResponse};
use base64::Engine;
use std::sync::Arc;

/// DNS-over-HTTPS client
pub struct DohClient {
    client: reqwest::Client,
    endpoint: String,
    enabled: bool,
}

/// DoH configuration
#[derive(Debug, Clone)]
pub struct DohConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub timeout: std::time::Duration,
}

impl Default for DohConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "https://cloudflare-dns.com/dns-query".to_string(),
            timeout: std::time::Duration::from_secs(5),
        }
    }
}

/// Well-known DoH providers
pub mod providers {
    pub const CLOUDFLARE: &str = "https://cloudflare-dns.com/dns-query";
    pub const GOOGLE: &str = "https://dns.google/dns-query";
    pub const QUAD9: &str = "https://dns.quad9.net/dns-query";
    pub const OPENDNS: &str = "https://doh.opendns.com/dns-query";
}

impl DohClient {
    /// Create new DoH client
    pub fn new(config: DohConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .use_rustls_tls()
            .build()
            .map_err(|e| VerySlipError::Network(format!("Failed to create DoH client: {}", e)))?;

        Ok(Self {
            client,
            endpoint: config.endpoint,
            enabled: config.enabled,
        })
    }

    /// Check if DoH is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Send DNS query via DoH
    pub async fn query(&self, query: &DnsQuery) -> Result<DnsResponse> {
        if !self.enabled {
            return Err(VerySlipError::InvalidState("DoH is not enabled".to_string()));
        }

        // Encode DNS query
        let query_data = query.encode()?;

        // Send POST request to DoH endpoint
        let response = self.client
            .post(&self.endpoint)
            .header("Content-Type", "application/dns-message")
            .header("Accept", "application/dns-message")
            .body(query_data)
            .send()
            .await
            .map_err(|e| VerySlipError::Network(format!("DoH request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(VerySlipError::Network(format!(
                "DoH request failed with status: {}",
                response.status()
            )));
        }

        // Parse DNS response
        let response_data = response
            .bytes()
            .await
            .map_err(|e| VerySlipError::Network(format!("Failed to read DoH response: {}", e)))?;

        DnsResponse::parse(&response_data)
    }

    /// Send DNS query via DoH with GET method (RFC 8484)
    pub async fn query_get(&self, query: &DnsQuery) -> Result<DnsResponse> {
        if !self.enabled {
            return Err(VerySlipError::InvalidState("DoH is not enabled".to_string()));
        }

        // Encode DNS query
        let query_data = query.encode()?;

        // Base64url encode the query
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&query_data);

        // Send GET request with dns parameter
        let url = format!("{}?dns={}", self.endpoint, encoded);
        
        let response = self.client
            .get(&url)
            .header("Accept", "application/dns-message")
            .send()
            .await
            .map_err(|e| VerySlipError::Network(format!("DoH GET request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(VerySlipError::Network(format!(
                "DoH GET request failed with status: {}",
                response.status()
            )));
        }

        // Parse DNS response
        let response_data = response
            .bytes()
            .await
            .map_err(|e| VerySlipError::Network(format!("Failed to read DoH response: {}", e)))?;

        DnsResponse::parse(&response_data)
    }
}

/// DoH client pool for load balancing across multiple providers
pub struct DohClientPool {
    clients: Vec<Arc<DohClient>>,
    next_index: std::sync::atomic::AtomicUsize,
}

impl DohClientPool {
    /// Create new DoH client pool
    pub fn new(configs: Vec<DohConfig>) -> Result<Self> {
        let clients = configs
            .into_iter()
            .map(|config| DohClient::new(config).map(Arc::new))
            .collect::<Result<Vec<_>>>()?;

        if clients.is_empty() {
            return Err(VerySlipError::InvalidState("No DoH clients configured".to_string()));
        }

        Ok(Self {
            clients,
            next_index: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// Get next client using round-robin
    pub fn next_client(&self) -> Arc<DohClient> {
        let index = self.next_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.clients[index % self.clients.len()].clone()
    }

    /// Send query using round-robin client selection
    pub async fn query(&self, query: &DnsQuery) -> Result<DnsResponse> {
        let client = self.next_client();
        client.query(query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doh_config_default() {
        let config = DohConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.endpoint, "https://cloudflare-dns.com/dns-query");
        assert_eq!(config.timeout, std::time::Duration::from_secs(5));
    }

    #[test]
    fn test_doh_client_creation() {
        let config = DohConfig::default();
        let client = DohClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_doh_client_disabled() {
        let config = DohConfig {
            enabled: false,
            ..Default::default()
        };
        let client = DohClient::new(config).unwrap();
        assert!(!client.is_enabled());
    }

    #[test]
    fn test_doh_providers() {
        assert_eq!(providers::CLOUDFLARE, "https://cloudflare-dns.com/dns-query");
        assert_eq!(providers::GOOGLE, "https://dns.google/dns-query");
        assert_eq!(providers::QUAD9, "https://dns.quad9.net/dns-query");
        assert_eq!(providers::OPENDNS, "https://doh.opendns.com/dns-query");
    }

    #[test]
    fn test_doh_client_pool() {
        let configs = vec![
            DohConfig {
                enabled: true,
                endpoint: providers::CLOUDFLARE.to_string(),
                timeout: std::time::Duration::from_secs(5),
            },
            DohConfig {
                enabled: true,
                endpoint: providers::GOOGLE.to_string(),
                timeout: std::time::Duration::from_secs(5),
            },
        ];

        let pool = DohClientPool::new(configs);
        assert!(pool.is_ok());
        
        let pool = pool.unwrap();
        let client1 = pool.next_client();
        let client2 = pool.next_client();
        
        // Should round-robin
        assert_ne!(client1.endpoint, client2.endpoint);
    }
}
