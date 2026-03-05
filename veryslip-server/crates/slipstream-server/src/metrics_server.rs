/// Metrics HTTP server for Prometheus integration
/// 
/// Provides HTTP endpoints:
/// - GET /metrics - Prometheus metrics in text format
/// - GET /health - Health check endpoint

use crate::metrics::MetricsCollector;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Metrics HTTP server
pub struct MetricsServer {
    collector: Arc<MetricsCollector>,
    port: u16,
}

impl MetricsServer {
    /// Create new metrics server
    pub fn new(collector: Arc<MetricsCollector>, port: u16) -> Self {
        Self { collector, port }
    }
    
    /// Start the metrics HTTP server
    pub async fn run(self) -> Result<(), std::io::Error> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await?;
        
        tracing::info!("Metrics server listening on http://{}", addr);
        tracing::info!("Endpoints: /metrics (Prometheus), /health (health check)");
        
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let collector = Arc::clone(&self.collector);
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, collector).await {
                            tracing::debug!("Metrics connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to accept metrics connection: {}", e);
                }
            }
        }
    }
}

/// Handle a single HTTP connection
async fn handle_connection(
    mut stream: TcpStream,
    collector: Arc<MetricsCollector>,
) -> Result<(), std::io::Error> {
    let mut buffer = vec![0u8; 1024];
    let n = stream.read(&mut buffer).await?;
    
    if n == 0 {
        return Ok(());
    }
    
    let request = String::from_utf8_lossy(&buffer[..n]);
    
    // Parse HTTP request line
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    
    if parts.len() < 2 {
        send_response(&mut stream, 400, "text/plain", "Bad Request").await?;
        return Ok(());
    }
    
    let method = parts[0];
    let path = parts[1];
    
    if method != "GET" {
        send_response(&mut stream, 405, "text/plain", "Method Not Allowed").await?;
        return Ok(());
    }
    
    match path {
        "/metrics" => {
            let metrics = collector.export_prometheus();
            send_response(&mut stream, 200, "text/plain; version=0.0.4", &metrics).await?;
        }
        "/health" => {
            send_response(&mut stream, 200, "text/plain", "OK").await?;
        }
        _ => {
            send_response(&mut stream, 404, "text/plain", "Not Found").await?;
        }
    }
    
    Ok(())
}

/// Send HTTP response
async fn send_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<(), std::io::Error> {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Unknown",
    };
    
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        status_text,
        content_type,
        body.len(),
        body
    );
    
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    
    async fn start_test_server(port: u16) -> Arc<MetricsCollector> {
        let collector = Arc::new(MetricsCollector::new());
        let server = MetricsServer::new(Arc::clone(&collector), port);
        
        tokio::spawn(async move {
            let _ = server.run().await;
        });
        
        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        collector
    }
    
    async fn send_request(port: u16, request: &str) -> Result<String, std::io::Error> {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).await?;
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;
        
        let mut response = String::new();
        stream.read_to_string(&mut response).await?;
        
        Ok(response)
    }
    
    #[tokio::test]
    async fn test_metrics_endpoint() {
        let port = 19090;
        let collector = start_test_server(port).await;
        
        // Record some metrics
        collector.record_bytes_sent(1000);
        collector.record_query("example.com", false);
        
        let request = "GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let response = send_request(port, request).await.unwrap();
        
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("veryslip_bytes_sent_total 1000"));
        assert!(response.contains("veryslip_queries_total 1"));
    }
    
    #[tokio::test]
    async fn test_health_endpoint() {
        let port = 19091;
        let _collector = start_test_server(port).await;
        
        let request = "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let response = send_request(port, request).await.unwrap();
        
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("OK"));
    }
    
    #[tokio::test]
    async fn test_not_found() {
        let port = 19092;
        let _collector = start_test_server(port).await;
        
        let request = "GET /notfound HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let response = send_request(port, request).await.unwrap();
        
        assert!(response.contains("HTTP/1.1 404 Not Found"));
    }
    
    #[tokio::test]
    async fn test_method_not_allowed() {
        let port = 19093;
        let _collector = start_test_server(port).await;
        
        let request = "POST /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let response = send_request(port, request).await.unwrap();
        
        assert!(response.contains("HTTP/1.1 405 Method Not Allowed"));
    }
}
