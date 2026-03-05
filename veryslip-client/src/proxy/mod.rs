use crate::{Result, VerySlipError};
use crate::pipeline::Pipeline;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};
use std::collections::HashMap;

/// HTTP proxy server
pub struct ProxyServer {
    config: ProxyConfig,
    stats: Arc<ProxyStats>,
    pipeline: Arc<Pipeline>,
}

/// Proxy configuration
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub bind_addr: String,
    pub max_connections: usize,
    pub idle_timeout: Duration,
    pub total_timeout: Duration,
    pub auth_enabled: bool,
    pub auth_username: Option<String>,
    pub auth_password: Option<String>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8080".to_string(),
            max_connections: 1000,
            idle_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(300),
            auth_enabled: false,
            auth_username: None,
            auth_password: None,
        }
    }
}

/// Proxy statistics
#[derive(Debug, Default)]
pub struct ProxyStats {
    pub connections_total: AtomicU64,
    pub connections_active: AtomicU64,
    pub requests_total: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub auth_failures: AtomicU64,
}

/// HTTP request
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub uri: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// HTTP response
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub reason: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl ProxyServer {
    /// Create new proxy server
    pub fn new(config: ProxyConfig, pipeline: Arc<Pipeline>) -> Self {
        Self {
            config,
            stats: Arc::new(ProxyStats::default()),
            pipeline,
        }
    }

    /// Start proxy server
    pub async fn start(&self) -> Result<()> {
        let listener = TcpListener::bind(&self.config.bind_addr).await
            .map_err(|e| VerySlipError::Network(format!("Failed to bind: {}", e)))?;

        tracing::info!("Proxy server listening on {}", self.config.bind_addr);

        loop {
            // Check connection limit
            if self.stats.connections_active.load(Ordering::Relaxed) >= self.config.max_connections as u64 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            let (stream, addr) = listener.accept().await
                .map_err(|e| VerySlipError::Network(format!("Accept failed: {}", e)))?;

            tracing::debug!("Accepted connection from {}", addr);

            let config = self.config.clone();
            let stats = self.stats.clone();
            let pipeline = self.pipeline.clone();

            tokio::spawn(async move {
                stats.connections_total.fetch_add(1, Ordering::Relaxed);
                stats.connections_active.fetch_add(1, Ordering::Relaxed);

                if let Err(e) = Self::handle_connection(stream, config, stats.clone(), pipeline).await {
                    tracing::warn!("Connection error: {}", e);
                }

                stats.connections_active.fetch_sub(1, Ordering::Relaxed);
            });
        }
    }

    /// Handle client connection
    async fn handle_connection(
        mut stream: TcpStream,
        config: ProxyConfig,
        stats: Arc<ProxyStats>,
        pipeline: Arc<Pipeline>,
    ) -> Result<()> {
        // Set timeouts
        let result = timeout(
            config.total_timeout,
            Self::process_connection(&mut stream, &config, &stats, &pipeline)
        ).await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(VerySlipError::Timeout),
        }
    }

    /// Process connection
    async fn process_connection(
        stream: &mut TcpStream,
        config: &ProxyConfig,
        stats: &Arc<ProxyStats>,
        pipeline: &Arc<Pipeline>,
    ) -> Result<()> {
        // Read request
        let request = Self::read_request(stream, config.idle_timeout).await?;
        stats.requests_total.fetch_add(1, Ordering::Relaxed);

        // Check authentication
        if config.auth_enabled {
            if !Self::check_auth(&request, config) {
                stats.auth_failures.fetch_add(1, Ordering::Relaxed);
                Self::send_auth_required(stream).await?;
                return Ok(());
            }
        }

        // Handle based on method
        match request.method.as_str() {
            "CONNECT" => Self::handle_connect(stream, request, stats, pipeline).await,
            "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS" | "PATCH" => {
                Self::handle_http(stream, request, stats, pipeline).await
            }
            _ => {
                Self::send_error(stream, 405, "Method Not Allowed").await?;
                Ok(())
            }
        }
    }

    /// Read HTTP request
    async fn read_request(stream: &mut TcpStream, idle_timeout: Duration) -> Result<HttpRequest> {
        let mut buffer = vec![0u8; 8192];
        let mut total_read = 0;

        // Read until we have headers
        loop {
            let n = timeout(idle_timeout, stream.read(&mut buffer[total_read..]))
                .await
                .map_err(|_| VerySlipError::Timeout)?
                .map_err(|e| VerySlipError::Network(format!("Read failed: {}", e)))?;

            if n == 0 {
                return Err(VerySlipError::ConnectionClosed);
            }

            total_read += n;

            // Check for end of headers
            if total_read >= 4 {
                let data = &buffer[..total_read];
                if let Some(pos) = Self::find_header_end(data) {
                    // Parse request line and headers
                    let header_data = &data[..pos];
                    let body_start = pos + 4;

                    let (method, uri, version, headers) = Self::parse_headers(header_data)?;

                    // Read body if Content-Length present
                    let mut body = Vec::new();
                    if let Some(content_length) = headers.get("content-length") {
                        if let Ok(len) = content_length.parse::<usize>() {
                            body.resize(len, 0);
                            let body_read = total_read - body_start;
                            body[..body_read].copy_from_slice(&data[body_start..total_read]);

                            // Read remaining body
                            if body_read < len {
                                stream.read_exact(&mut body[body_read..]).await
                                    .map_err(|e| VerySlipError::Network(format!("Body read failed: {}", e)))?;
                            }
                        }
                    }

                    return Ok(HttpRequest {
                        method,
                        uri,
                        version,
                        headers,
                        body,
                    });
                }
            }

            if total_read >= buffer.len() {
                buffer.resize(buffer.len() * 2, 0);
            }
        }
    }

    /// Find end of HTTP headers
    fn find_header_end(data: &[u8]) -> Option<usize> {
        for i in 0..data.len().saturating_sub(3) {
            if &data[i..i+4] == b"\r\n\r\n" {
                return Some(i);
            }
        }
        None
    }

    /// Parse HTTP headers
    fn parse_headers(data: &[u8]) -> Result<(String, String, String, HashMap<String, String>)> {
        let text = std::str::from_utf8(data)
            .map_err(|e| VerySlipError::Parse(format!("Invalid UTF-8: {}", e)))?;

        let mut lines = text.lines();
        
        // Parse request line
        let request_line = lines.next()
            .ok_or_else(|| VerySlipError::Parse("Empty request".to_string()))?;
        
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(VerySlipError::Parse("Invalid request line".to_string()));
        }

        let method = parts[0].to_string();
        let uri = parts[1].to_string();
        let version = parts[2].to_string();

        // Parse headers
        let mut headers = HashMap::new();
        for line in lines {
            if let Some(pos) = line.find(':') {
                let key = line[..pos].trim().to_lowercase();
                let value = line[pos+1..].trim().to_string();
                headers.insert(key, value);
            }
        }

        Ok((method, uri, version, headers))
    }

    /// Check authentication
    fn check_auth(request: &HttpRequest, config: &ProxyConfig) -> bool {
        if let Some(auth_header) = request.headers.get("proxy-authorization") {
            if let Some(username) = &config.auth_username {
                if let Some(password) = &config.auth_password {
                    // Parse Basic auth
                    if let Some(encoded) = auth_header.strip_prefix("Basic ") {
                        use base64::Engine;
                        let engine = base64::engine::general_purpose::STANDARD;
                        if let Ok(decoded) = engine.decode(encoded) {
                            if let Ok(credentials) = String::from_utf8(decoded) {
                                let expected = format!("{}:{}", username, password);
                                return credentials == expected;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// Send 407 Proxy Authentication Required
    async fn send_auth_required(stream: &mut TcpStream) -> Result<()> {
        let response = b"HTTP/1.1 407 Proxy Authentication Required\r\n\
                        Proxy-Authenticate: Basic realm=\"Very Slip Proxy\"\r\n\
                        Content-Length: 0\r\n\
                        \r\n";
        
        stream.write_all(response).await
            .map_err(|e| VerySlipError::Network(format!("Write failed: {}", e)))?;
        
        Ok(())
    }

    /// Send error response
    async fn send_error(stream: &mut TcpStream, status: u16, reason: &str) -> Result<()> {
        let body = format!("{} {}", status, reason);
        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\n\r\n{}",
            status, reason, body.len(), body
        );
        
        stream.write_all(response.as_bytes()).await
            .map_err(|e| VerySlipError::Network(format!("Write failed: {}", e)))?;
        
        Ok(())
    }

    /// Handle CONNECT method (HTTPS tunneling)
    async fn handle_connect(
        stream: &mut TcpStream,
        request: HttpRequest,
        stats: &Arc<ProxyStats>,
        pipeline: &Arc<Pipeline>,
    ) -> Result<()> {
        // Parse target host:port
        let parts: Vec<&str> = request.uri.split(':').collect();
        if parts.len() != 2 {
            Self::send_error(stream, 400, "Bad Request").await?;
            return Ok(());
        }

        let host = parts[0];
        let port = parts[1].parse::<u16>()
            .map_err(|_| VerySlipError::Parse("Invalid port".to_string()))?;

        tracing::debug!("CONNECT tunnel requested for {}:{}", host, port);

        // Send 200 Connection Established
        let response = b"HTTP/1.1 200 Connection Established\r\n\r\n";
        stream.write_all(response).await
            .map_err(|e| VerySlipError::Network(format!("Write failed: {}", e)))?;

        stats.bytes_sent.fetch_add(response.len() as u64, Ordering::Relaxed);

        // Establish connection to target through DNS tunnel
        let target_addr = format!("{}:{}", host, port);
        
        // Create tunnel through pipeline
        match pipeline.process_connect_tunnel(target_addr.clone(), stream).await {
            Ok(_) => {
                tracing::info!("CONNECT tunnel established for {}", target_addr);
                Ok(())
            }
            Err(e) => {
                tracing::error!("CONNECT tunnel failed for {}: {}", target_addr, e);
                Err(e)
            }
        }
    }

    /// Handle HTTP methods (GET, POST, etc.)
    async fn handle_http(
        stream: &mut TcpStream,
        request: HttpRequest,
        stats: &Arc<ProxyStats>,
        pipeline: &Arc<Pipeline>,
    ) -> Result<()> {
        tracing::debug!("HTTP request: {} {}", request.method, request.uri);

        // Convert headers to vec of tuples
        let headers: Vec<(String, String)> = request.headers.into_iter().collect();
        let body_len = request.body.len();

        // Send request through DNS tunnel via pipeline
        let response = pipeline.process_request(
            request.method,
            request.uri,
            headers,
            request.body,
        ).await?;

        // Build HTTP response
        let mut response_data = format!("HTTP/1.1 {} OK\r\n", response.status).into_bytes();
        
        for (key, value) in &response.headers {
            response_data.extend_from_slice(format!("{}: {}\r\n", key, value).as_bytes());
        }
        
        response_data.extend_from_slice(format!("Content-Length: {}\r\n\r\n", response.body.len()).as_bytes());
        response_data.extend_from_slice(&response.body);

        // Send response to client
        stream.write_all(&response_data).await
            .map_err(|e| VerySlipError::Network(format!("Write failed: {}", e)))?;

        // Update stats
        stats.bytes_sent.fetch_add(response_data.len() as u64, Ordering::Relaxed);
        stats.bytes_received.fetch_add(body_len as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> ProxyStats {
        ProxyStats {
            connections_total: AtomicU64::new(self.stats.connections_total.load(Ordering::Relaxed)),
            connections_active: AtomicU64::new(self.stats.connections_active.load(Ordering::Relaxed)),
            requests_total: AtomicU64::new(self.stats.requests_total.load(Ordering::Relaxed)),
            bytes_sent: AtomicU64::new(self.stats.bytes_sent.load(Ordering::Relaxed)),
            bytes_received: AtomicU64::new(self.stats.bytes_received.load(Ordering::Relaxed)),
            auth_failures: AtomicU64::new(self.stats.auth_failures.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_config_default() {
        let config = ProxyConfig::default();
        assert_eq!(config.bind_addr, "127.0.0.1:8080");
        assert_eq!(config.max_connections, 1000);
        assert!(!config.auth_enabled);
    }

    #[test]
    fn test_find_header_end() {
        let data = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        assert_eq!(ProxyServer::find_header_end(data), Some(33));

        let incomplete = b"GET / HTTP/1.1\r\nHost: example.com\r\n";
        assert_eq!(ProxyServer::find_header_end(incomplete), None);
    }

    #[test]
    fn test_parse_headers() {
        let data = b"GET /path HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n";
        let (method, uri, version, headers) = ProxyServer::parse_headers(data).unwrap();
        
        assert_eq!(method, "GET");
        assert_eq!(uri, "/path");
        assert_eq!(version, "HTTP/1.1");
        assert_eq!(headers.get("host"), Some(&"example.com".to_string()));
        assert_eq!(headers.get("user-agent"), Some(&"test".to_string()));
    }

    #[test]
    fn test_parse_connect_request() {
        let data = b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n";
        let (method, uri, version, _) = ProxyServer::parse_headers(data).unwrap();
        
        assert_eq!(method, "CONNECT");
        assert_eq!(uri, "example.com:443");
        assert_eq!(version, "HTTP/1.1");
    }

    #[test]
    fn test_check_auth_valid() {
        let config = ProxyConfig {
            auth_enabled: true,
            auth_username: Some("user".to_string()),
            auth_password: Some("pass".to_string()),
            ..Default::default()
        };

        let mut headers = HashMap::new();
        // "user:pass" in base64 is "dXNlcjpwYXNz"
        headers.insert("proxy-authorization".to_string(), "Basic dXNlcjpwYXNz".to_string());

        let request = HttpRequest {
            method: "GET".to_string(),
            uri: "/".to_string(),
            version: "HTTP/1.1".to_string(),
            headers,
            body: vec![],
        };

        assert!(ProxyServer::check_auth(&request, &config));
    }

    #[test]
    fn test_check_auth_invalid() {
        let config = ProxyConfig {
            auth_enabled: true,
            auth_username: Some("user".to_string()),
            auth_password: Some("pass".to_string()),
            ..Default::default()
        };

        let mut headers = HashMap::new();
        headers.insert("proxy-authorization".to_string(), "Basic invalid".to_string());

        let request = HttpRequest {
            method: "GET".to_string(),
            uri: "/".to_string(),
            version: "HTTP/1.1".to_string(),
            headers,
            body: vec![],
        };

        assert!(!ProxyServer::check_auth(&request, &config));
    }

    #[test]
    fn test_check_auth_missing() {
        let config = ProxyConfig {
            auth_enabled: true,
            auth_username: Some("user".to_string()),
            auth_password: Some("pass".to_string()),
            ..Default::default()
        };

        let request = HttpRequest {
            method: "GET".to_string(),
            uri: "/".to_string(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            body: vec![],
        };

        assert!(!ProxyServer::check_auth(&request, &config));
    }

    #[test]
    fn test_proxy_stats() {
        let stats = ProxyStats::default();
        assert_eq!(stats.connections_total.load(Ordering::Relaxed), 0);
        assert_eq!(stats.requests_total.load(Ordering::Relaxed), 0);
    }
}
