use crate::{Result, VerySlipError};
use crate::filter::FilterEngine;
use crate::cache::CacheManager;
use crate::priority::{PriorityQueue, Priority, PendingRequest, HttpResponse};
use crate::compression::CompressionEngine;
use crate::query::QueryEngine;
use crate::load_balancer::LoadBalancer;
use crate::connection::ConnectionPool;
use crate::prefetch::PrefetchEngine;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Instant;

/// Request pipeline that wires all components together
pub struct Pipeline {
    filter: Arc<FilterEngine>,
    cache: Arc<CacheManager>,
    priority_queue: Arc<PriorityQueue>,
    compression: Arc<CompressionEngine>,
    query_engine: Arc<QueryEngine>,
    load_balancer: Arc<LoadBalancer>,
    connection_pool: Arc<ConnectionPool>,
    prefetch: Arc<PrefetchEngine>,
}

impl Pipeline {
    /// Create new pipeline
    pub fn new(
        filter: Arc<FilterEngine>,
        cache: Arc<CacheManager>,
        priority_queue: Arc<PriorityQueue>,
        compression: Arc<CompressionEngine>,
        query_engine: Arc<QueryEngine>,
        load_balancer: Arc<LoadBalancer>,
        connection_pool: Arc<ConnectionPool>,
        prefetch: Arc<PrefetchEngine>,
    ) -> Self {
        Self {
            filter,
            cache,
            priority_queue,
            compression,
            query_engine,
            load_balancer,
            connection_pool,
            prefetch,
        }
    }

    /// Process HTTP request through the pipeline
    pub async fn process_request(
        &self,
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Result<HttpResponse> {
        // Step 1: Check filter/blocklist
        // Extract host from URL
        let host = if let Ok(parsed_url) = url::Url::parse(&url) {
            parsed_url.host_str().unwrap_or("").to_string()
        } else {
            url.clone()
        };
        
        if let Some(reason) = self.filter.should_block(&host) {
            tracing::debug!("Blocked request to {} (reason: {:?})", url, reason);
            return Ok(HttpResponse {
                status: 204,
                headers: vec![],
                body: vec![],
            });
        }

        // Step 2: Check cache
        if method == "GET" {
            let cache_key = crate::cache::CacheKey {
                url: url.clone(),
                vary_headers: vec![],
            };
            
            if let Some(cached) = self.cache.get(&cache_key) {
                tracing::debug!("Cache hit for {}", url);
                
                // Trigger prefetch for HTML responses
                if Self::is_html_response(&cached.headers) {
                    let prefetch = self.prefetch.clone();
                    let url_clone = url.clone();
                    let body_clone = cached.body.clone();
                    tokio::spawn(async move {
                        if let Ok(html) = String::from_utf8(body_clone) {
                            if let Ok(base_url) = url::Url::parse(&url_clone) {
                                if let Err(e) = prefetch.extract_resources(&html, &base_url) {
                                    tracing::warn!("Prefetch failed: {}", e);
                                }
                            }
                        }
                    });
                }

                return Ok(HttpResponse {
                    status: cached.status,
                    headers: cached.headers,
                    body: cached.body,
                });
            }
        }

        // Step 3: Classify priority and enqueue
        let priority = Priority::classify(&url, &method, &headers);
        
        let (response_tx, response_rx) = oneshot::channel();
        let request = PendingRequest {
            url: url.clone(),
            method: method.clone(),
            headers: headers.clone(),
            body: body.clone(),
            priority,
            enqueued_at: Instant::now(),
            response_tx,
        };

        self.priority_queue.enqueue(request).await?;

        // Step 4: Process from priority queue using worker task
        // Spawn a dedicated worker that processes this specific request
        let priority_queue = self.priority_queue.clone();
        let compression = self.compression.clone();
        let query_engine = self.query_engine.clone();
        let cache = self.cache.clone();
        let prefetch = self.prefetch.clone();
        
        tokio::spawn(async move {
            if let Some(pending) = priority_queue.dequeue().await {
                let response = Self::process_request_worker(
                    pending,
                    compression,
                    query_engine,
                    cache,
                    prefetch,
                ).await;
                
                match response {
                    Ok(resp) => {
                        tracing::debug!("Worker processed request successfully");
                        // Response sent through oneshot channel
                    }
                    Err(e) => {
                        tracing::error!("Worker failed to process request: {}", e);
                    }
                }
            }
        });

        // Wait for response
        response_rx.await
            .map_err(|_| VerySlipError::InvalidState("Response channel closed".to_string()))
    }

    /// Worker function to process a pending request
    async fn process_request_worker(
        mut request: PendingRequest,
        compression: Arc<CompressionEngine>,
        query_engine: Arc<QueryEngine>,
        cache: Arc<CacheManager>,
        prefetch: Arc<PrefetchEngine>,
    ) -> Result<HttpResponse> {
        // Compress request payload
        let compressed_body = compression.compress(&request.body, Some("application/octet-stream"))?;

        // Send through DNS tunnel
        // Build HTTP request packet
        let request_data = Self::build_http_request(
            &request.method,
            &request.url,
            &request.headers,
            &compressed_body,
        );

        // Send via query engine
        query_engine.send_data(request_data).await?;

        // Receive response from query engine
        // Poll reorder buffer for response data
        let response_data = loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            if let Some(data) = query_engine.get_ordered_data().await {
                break data;
            }
            // Timeout after 5 seconds
            if request.enqueued_at.elapsed() > tokio::time::Duration::from_secs(5) {
                return Err(VerySlipError::Timeout);
            }
        };

        // Decompress response
        let decompressed = compression.decompress(&response_data)?;

        // Parse HTTP response
        let response = Self::parse_http_response(&decompressed)?;

        // Store in cache if cacheable
        if request.method == "GET" && cache.should_cache(&request.method, response.status, &response.headers) {
            let cache_key = crate::cache::CacheKey {
                url: request.url.clone(),
                vary_headers: vec![],
            };
            let cache_entry = crate::cache::CacheEntry {
                url: request.url.clone(),
                status: response.status,
                headers: response.headers.clone(),
                body: response.body.clone(),
                stored_at: std::time::Instant::now(),
                expires_at: Some(std::time::Instant::now() + std::time::Duration::from_secs(3600)),
                etag: None,
                last_modified: None,
            };
            cache.put(cache_key, cache_entry)?;
        }

        // Trigger prefetch for HTML responses
        if Self::is_html_response(&response.headers) {
            let url_clone = request.url.clone();
            let body_clone = response.body.clone();
            tokio::spawn(async move {
                if let Ok(html) = String::from_utf8(body_clone) {
                    if let Ok(base_url) = url::Url::parse(&url_clone) {
                        if let Err(e) = prefetch.extract_resources(&html, &base_url) {
                            tracing::warn!("Prefetch failed: {}", e);
                        }
                    }
                }
            });
        }

        // Send response through channel
        let _ = request.response_tx.send(response.clone());

        Ok(response)
    }

    /// Process a pending request from the priority queue (used by main thread)
    async fn process_pending_request(&self, request: PendingRequest) -> Result<HttpResponse> {
        Self::process_request_worker(
            request,
            self.compression.clone(),
            self.query_engine.clone(),
            self.cache.clone(),
            self.prefetch.clone(),
        ).await
    }

    /// Build HTTP request packet
    fn build_http_request(
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Vec<u8> {
        let mut request = format!("{} {} HTTP/1.1\r\n", method, url);
        
        for (key, value) in headers {
            request.push_str(&format!("{}: {}\r\n", key, value));
        }
        
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        request.push_str("\r\n");
        
        let mut data = request.into_bytes();
        data.extend_from_slice(body);
        data
    }

    /// Parse HTTP response
    fn parse_http_response(data: &[u8]) -> Result<HttpResponse> {
        // Find end of headers
        let header_end = data.windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| VerySlipError::Parse("Invalid HTTP response".to_string()))?;

        let header_data = &data[..header_end];
        let body = &data[header_end + 4..];

        // Parse status line and headers
        let header_text = std::str::from_utf8(header_data)
            .map_err(|e| VerySlipError::Parse(format!("Invalid UTF-8: {}", e)))?;

        let mut lines = header_text.lines();
        
        // Parse status line
        let status_line = lines.next()
            .ok_or_else(|| VerySlipError::Parse("Empty response".to_string()))?;
        
        let parts: Vec<&str> = status_line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(VerySlipError::Parse("Invalid status line".to_string()));
        }

        let status = parts[1].parse::<u16>()
            .map_err(|e| VerySlipError::Parse(format!("Invalid status code: {}", e)))?;

        // Parse headers
        let mut headers = Vec::new();
        for line in lines {
            if let Some(pos) = line.find(':') {
                let key = line[..pos].trim().to_string();
                let value = line[pos+1..].trim().to_string();
                headers.push((key, value));
            }
        }

        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }

    /// Check if response is HTML
    fn is_html_response(headers: &[(String, String)]) -> bool {
        for (key, value) in headers {
            if key.eq_ignore_ascii_case("content-type") {
                return value.contains("text/html");
            }
        }
        false
    }

    /// Process CONNECT tunnel (HTTPS)
    pub async fn process_connect_tunnel(
        &self,
        target: String,
        client_stream: &mut tokio::net::TcpStream,
    ) -> Result<()> {
        // Parse target host:port
        let parts: Vec<&str> = target.split(':').collect();
        if parts.len() != 2 {
            return Err(VerySlipError::Parse("Invalid CONNECT target".to_string()));
        }

        // Establish QUIC stream through connection pool
        let tunnel_data = format!("CONNECT {} HTTP/1.1\r\n\r\n", target).into_bytes();
        let stream_id = self.connection_pool.send_stream(tunnel_data).await?;

        // Get connection ID for receiving
        let conn = self.connection_pool.get_connection().await?;
        let conn_id = conn.id;

        // Bidirectional relay
        let connection_pool = self.connection_pool.clone();
        
        let mut buf = vec![0u8; 8192];
        let (mut read_half, mut write_half) = client_stream.split();
        
        loop {
            tokio::select! {
                // Client -> Tunnel
                result = read_half.read(&mut buf) => {
                    match result {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            if let Err(e) = connection_pool.send_stream(buf[..n].to_vec()).await {
                                tracing::error!("Tunnel send failed: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Client read failed: {}", e);
                            break;
                        }
                    }
                }
                // Tunnel -> Client
                result = connection_pool.recv_stream(conn_id, stream_id) => {
                    match result {
                        Ok(data) if !data.is_empty() => {
                            if let Err(e) = write_half.write_all(&data).await {
                                tracing::error!("Client write failed: {}", e);
                                break;
                            }
                        }
                        Ok(_) => break, // Empty response, connection closed
                        Err(e) => {
                            tracing::error!("Tunnel recv failed: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_pipeline() -> Pipeline {
        let filter = Arc::new(FilterEngine::new());

        let cache_config = crate::cache::CacheConfig::default();
        let cache = Arc::new(CacheManager::new(cache_config).unwrap());

        let priority_config = crate::priority::PriorityConfig::default();
        let priority_queue = Arc::new(PriorityQueue::new(priority_config));

        let compression_config = crate::compression::CompressionConfig::default();
        let compression = Arc::new(CompressionEngine::new(compression_config).unwrap());

        let domains = vec!["tunnel1.example.com".to_string()];
        let lb_config = crate::load_balancer::LoadBalancerConfig::default();
        let load_balancer = Arc::new(LoadBalancer::new(domains, lb_config));

        let buffer_config = crate::buffer::BufferPoolConfig::default();
        let buffer_pool = Arc::new(crate::buffer::BufferPool::new(buffer_config));

        let query_config = crate::query::QueryConfig::default();
        let query_engine = Arc::new(QueryEngine::new(query_config, load_balancer.clone(), buffer_pool));

        let connection_config = crate::connection::ConnectionPoolConfig::default();
        let connection_pool = Arc::new(ConnectionPool::new(connection_config, "127.0.0.1:4433".to_string()).unwrap());

        let prefetch_config = crate::prefetch::PrefetchConfig::default();
        let prefetch = Arc::new(PrefetchEngine::new(
            prefetch_config,
            cache.clone(),
            priority_queue.clone(),
        ));

        Pipeline::new(
            filter,
            cache,
            priority_queue,
            compression,
            query_engine,
            load_balancer,
            connection_pool,
            prefetch,
        )
    }

    #[test]
    fn test_build_http_request() {
        let headers = vec![
            ("Host".to_string(), "example.com".to_string()),
            ("User-Agent".to_string(), "test".to_string()),
        ];
        let body = b"test body";

        let request = Pipeline::build_http_request("GET", "/path", &headers, body);
        let request_str = String::from_utf8_lossy(&request);

        assert!(request_str.contains("GET /path HTTP/1.1"));
        assert!(request_str.contains("Host: example.com"));
        assert!(request_str.contains("User-Agent: test"));
        assert!(request_str.contains("Content-Length: 9"));
        assert!(request_str.ends_with("test body"));
    }

    #[test]
    fn test_parse_http_response() {
        let response_data = b"HTTP/1.1 200 OK\r\n\
                             Content-Type: text/html\r\n\
                             Content-Length: 11\r\n\
                             \r\n\
                             Hello World";

        let response = Pipeline::parse_http_response(response_data).unwrap();
        
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"Hello World");
        
        let content_type = response.headers.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str());
        assert_eq!(content_type, Some("text/html"));
    }

    #[test]
    fn test_is_html_response() {
        let headers = vec![
            ("Content-Type".to_string(), "text/html; charset=utf-8".to_string()),
        ];
        assert!(Pipeline::is_html_response(&headers));

        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        assert!(!Pipeline::is_html_response(&headers));
    }

    #[tokio::test]
    async fn test_blocked_request() {
        let pipeline = create_test_pipeline();
        
        // Add a domain to blocklist
        pipeline.filter.add_to_blocklist("ads.example.com".to_string(), crate::filter::BlockReason::Advertisement);

        let response = pipeline.process_request(
            "GET".to_string(),
            "https://ads.example.com/ad.js".to_string(),
            vec![],
            vec![],
        ).await.unwrap();

        assert_eq!(response.status, 204);
    }

    #[tokio::test]
    async fn test_pipeline_creation() {
        let _pipeline = create_test_pipeline();
        // Just verify pipeline can be created
    }
}
