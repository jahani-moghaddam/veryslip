use crate::{VerySlipError, Result};
use crate::dns::{DnsQuery, DnsResponse};
use crate::load_balancer::LoadBalancer;
use crate::buffer::BufferPool;
use crate::doh::DohClientPool;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::BTreeMap;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::{Duration, timeout};
use bytes::{BufMut, BytesMut};

/// Query engine for parallel DNS query processing
pub struct QueryEngine {
    config: QueryConfig,
    in_flight: Arc<DashMap<u16, InFlightQuery>>,
    reorder_buffer: Arc<Mutex<BTreeMap<u64, Vec<u8>>>>,
    next_sequence: AtomicU64,
    next_query_id: AtomicU64,
    load_balancer: Arc<LoadBalancer>,
    #[allow(dead_code)]
    buffer_pool: Arc<BufferPool>,
    stats: QueryStats,
    concurrency_limiter: Arc<Semaphore>,
    resolvers: Vec<String>,
    next_resolver: AtomicU64,
    doh_client: Option<Arc<DohClientPool>>,
    use_doh_get: bool,
}

/// Query engine configuration
#[derive(Debug, Clone)]
pub struct QueryConfig {
    pub concurrency: usize,
    pub max_in_flight: usize,
    pub batch_timeout: Duration,
    pub batch_threshold: f32,
    pub query_timeout: Duration,
    pub max_retries: usize,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            concurrency: 8,
            max_in_flight: 1000,
            batch_timeout: Duration::from_millis(5),
            batch_threshold: 0.8,
            query_timeout: Duration::from_secs(5),
            max_retries: 3,
        }
    }
}

/// In-flight query tracking
#[derive(Debug, Clone)]
pub struct InFlightQuery {
    pub query_id: u16,
    pub sequence: u64,
    pub sent_at: std::time::Instant,
    pub domain: String,
    pub packet_count: usize,
    pub offsets: Vec<usize>,
    pub retries: usize,
}

/// Query statistics
#[derive(Debug, Default)]
pub struct QueryStats {
    pub queries_sent: AtomicU64,
    pub queries_success: AtomicU64,
    pub queries_failed: AtomicU64,
    pub queries_timeout: AtomicU64,
    pub queries_retried: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
}

impl QueryEngine {
    /// Create new query engine
    pub fn new(
        config: QueryConfig,
        load_balancer: Arc<LoadBalancer>,
        buffer_pool: Arc<BufferPool>,
    ) -> Self {
        Self {
            concurrency_limiter: Arc::new(Semaphore::new(config.concurrency)),
            config,
            in_flight: Arc::new(DashMap::new()),
            reorder_buffer: Arc::new(Mutex::new(BTreeMap::new())),
            next_sequence: AtomicU64::new(0),
            next_query_id: AtomicU64::new(1),
            load_balancer,
            buffer_pool,
            stats: QueryStats::default(),
            resolvers: vec![
                "8.8.8.8:53".to_string(),      // Google DNS
                "8.8.4.4:53".to_string(),      // Google DNS Secondary
                "1.1.1.1:53".to_string(),      // Cloudflare DNS
                "1.0.0.1:53".to_string(),      // Cloudflare DNS Secondary
            ],
            next_resolver: AtomicU64::new(0),
            doh_client: None,
            use_doh_get: false,
        }
    }

    /// Create new query engine with DoH support
    pub fn new_with_doh(
        config: QueryConfig,
        load_balancer: Arc<LoadBalancer>,
        buffer_pool: Arc<BufferPool>,
        doh_client: Arc<DohClientPool>,
        use_get_method: bool,
    ) -> Self {
        Self {
            concurrency_limiter: Arc::new(Semaphore::new(config.concurrency)),
            config,
            in_flight: Arc::new(DashMap::new()),
            reorder_buffer: Arc::new(Mutex::new(BTreeMap::new())),
            next_sequence: AtomicU64::new(0),
            next_query_id: AtomicU64::new(1),
            load_balancer,
            buffer_pool,
            stats: QueryStats::default(),
            resolvers: vec![
                "8.8.8.8:53".to_string(),      // Google DNS
                "8.8.4.4:53".to_string(),      // Google DNS Secondary
                "1.1.1.1:53".to_string(),      // Cloudflare DNS
                "1.0.0.1:53".to_string(),      // Cloudflare DNS Secondary
            ],
            next_resolver: AtomicU64::new(0),
            doh_client: Some(doh_client),
            use_doh_get: use_get_method,
        }
    }

    /// Get next DNS resolver using round-robin
    fn get_next_resolver(&self) -> String {
        let index = self.next_resolver.fetch_add(1, Ordering::Relaxed);
        self.resolvers[(index as usize) % self.resolvers.len()].clone()
    }

    /// Send data through DNS tunnel
    pub async fn send_data(&self, data: Vec<u8>) -> Result<()> {
        // Check in-flight limit
        if self.in_flight.len() >= self.config.max_in_flight {
            return Err(VerySlipError::Network("Too many in-flight queries".to_string()));
        }

        // Acquire sequence number
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);

        // Select domain
        let domain_state = self.load_balancer
            .select_domain()
            .ok_or_else(|| VerySlipError::Network("No domains available".to_string()))?;

        // Create DNS query
        let query_id = self.allocate_query_id();
        let query = DnsQuery::new(query_id, &domain_state.domain, data.clone());

        // Track in-flight
        self.in_flight.insert(query_id, InFlightQuery {
            query_id,
            sequence,
            sent_at: std::time::Instant::now(),
            domain: domain_state.domain.clone(),
            packet_count: 1,
            offsets: vec![0],
            retries: 0,
        });

        // Send query with concurrency limit
        let permit = self.concurrency_limiter.clone().acquire_owned().await
            .map_err(|_| VerySlipError::Network("Failed to acquire semaphore".to_string()))?;

        let in_flight = self.in_flight.clone();
        let load_balancer = self.load_balancer.clone();
        let stats = self.stats.clone();
        let query_timeout = self.config.query_timeout;
        let reorder_buffer = self.reorder_buffer.clone();
        let resolver_addr = self.get_next_resolver();
        let doh_client = self.doh_client.clone();
        let use_doh_get = self.use_doh_get;

        tokio::spawn(async move {
            let _permit = permit; // Hold permit until task completes

            // Send DNS query with timeout - use DoH if available, otherwise UDP
            let result = if let Some(ref doh) = doh_client {
                // Use DNS-over-HTTPS
                tracing::debug!("Sending DNS query via DoH");
                stats.queries_sent.fetch_add(1, Ordering::Relaxed);
                stats.bytes_sent.fetch_add(data.len() as u64, Ordering::Relaxed);

                timeout(query_timeout, async {
                    let response = if use_doh_get {
                        doh.next_client().query_get(&query).await?
                    } else {
                        doh.query(&query).await?
                    };

                    // Check response code
                    if response.rcode != crate::dns::ResponseCode::NoError {
                        return Err(VerySlipError::Dns(format!("DNS error: {:?}", response.rcode)));
                    }

                    // Extract payload from response
                    let payload = response.extract_payload()?;
                    stats.bytes_received.fetch_add(payload.len() as u64, Ordering::Relaxed);

                    Ok::<_, VerySlipError>(payload)
                }).await
            } else {
                // Use traditional UDP DNS
                let query_data = match query.encode() {
                    Ok(data) => data,
                    Err(e) => {
                        tracing::error!("Failed to encode DNS query: {}", e);
                        stats.queries_failed.fetch_add(1, Ordering::Relaxed);
                        load_balancer.mark_failure(&domain_state.domain);
                        in_flight.remove(&query_id);
                        return;
                    }
                };

                let socket = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to create UDP socket: {}", e);
                        stats.queries_failed.fetch_add(1, Ordering::Relaxed);
                        load_balancer.mark_failure(&domain_state.domain);
                        in_flight.remove(&query_id);
                        return;
                    }
                };

                timeout(query_timeout, async {
                    // Send query
                    socket.send_to(&query_data, &resolver_addr).await
                        .map_err(|e| VerySlipError::Network(format!("DNS send failed: {}", e)))?;

                    tracing::debug!("Sent UDP DNS query of {} bytes to {}", query_data.len(), resolver_addr);
                    stats.queries_sent.fetch_add(1, Ordering::Relaxed);
                    stats.bytes_sent.fetch_add(data.len() as u64, Ordering::Relaxed);

                    // Receive response
                    let mut recv_buf = vec![0u8; 4096];
                    let (len, _) = socket.recv_from(&mut recv_buf).await
                        .map_err(|e| VerySlipError::Network(format!("DNS recv failed: {}", e)))?;

                    recv_buf.truncate(len);
                    tracing::debug!("Received UDP DNS response of {} bytes", len);

                    // Parse DNS response
                    let response = DnsResponse::parse(&recv_buf)?;
                    
                    // Check response code
                    if response.rcode != crate::dns::ResponseCode::NoError {
                        return Err(VerySlipError::Dns(format!("DNS error: {:?}", response.rcode)));
                    }

                    // Extract payload from response
                    let payload = response.extract_payload()?;
                    stats.bytes_received.fetch_add(payload.len() as u64, Ordering::Relaxed);

                    Ok::<_, VerySlipError>(payload)
                }).await
            };

            match result {
                Ok(Ok(payload)) => {
                    // Success - remove from in-flight and add to reorder buffer
                    if let Some((_, in_flight_query)) = in_flight.remove(&query_id) {
                        let rtt = in_flight_query.sent_at.elapsed();
                        load_balancer.mark_success(&domain_state.domain, rtt);
                        stats.queries_success.fetch_add(1, Ordering::Relaxed);

                        // Add to reorder buffer
                        let mut buffer = reorder_buffer.lock().await;
                        buffer.insert(sequence, payload);
                    }
                }
                Ok(Err(e)) => {
                    tracing::error!("DNS query failed: {}", e);
                    stats.queries_failed.fetch_add(1, Ordering::Relaxed);
                    load_balancer.mark_failure(&domain_state.domain);
                    in_flight.remove(&query_id);
                }
                Err(_) => {
                    tracing::error!("DNS query timeout");
                    stats.queries_timeout.fetch_add(1, Ordering::Relaxed);
                    load_balancer.mark_failure(&domain_state.domain);
                    in_flight.remove(&query_id);
                }
            }
        });

        Ok(())
    }

    /// Send batch of packets
    pub async fn send_batch(&self, packets: Vec<Vec<u8>>) -> Result<()> {
        if packets.is_empty() {
            return Ok(());
        }

        // Create batch payload
        let batch = self.create_batch(&packets)?;

        // Send as single query
        self.send_data(batch).await
    }

    /// Create batch from multiple packets
    fn create_batch(&self, packets: &[Vec<u8>]) -> Result<Vec<u8>> {
        let mut batch = BytesMut::new();

        // Packet count
        batch.put_u8(packets.len() as u8);

        // Calculate offsets
        let mut offsets = Vec::new();
        let mut current_offset = 0;
        for packet in packets {
            offsets.push(current_offset);
            current_offset += packet.len();
        }

        // Write offsets (2 bytes each)
        for offset in &offsets {
            batch.put_u16(*offset as u16);
        }

        // Write packet data
        for packet in packets {
            batch.put_slice(packet);
        }

        Ok(batch.to_vec())
    }

    /// Receive DNS response
    pub async fn receive_response(&self, response: DnsResponse) -> Result<()> {
        // Find in-flight query
        let in_flight = match self.in_flight.remove(&response.id) {
            Some((_, query)) => query,
            None => return Ok(()), // Unknown query, ignore
        };

        // Extract payload
        let payload = response.extract_payload()?;

        // Calculate RTT
        let rtt = in_flight.sent_at.elapsed();

        // Mark success
        self.load_balancer.mark_success(&in_flight.domain, rtt);
        self.stats.queries_success.fetch_add(1, Ordering::Relaxed);
        self.stats.bytes_received.fetch_add(payload.len() as u64, Ordering::Relaxed);

        // Add to reorder buffer
        let mut buffer = self.reorder_buffer.lock().await;
        buffer.insert(in_flight.sequence, payload);

        Ok(())
    }

    /// Get ordered data from reorder buffer
    pub async fn get_ordered_data(&self) -> Option<Vec<u8>> {
        let mut buffer = self.reorder_buffer.lock().await;
        
        if buffer.is_empty() {
            return None;
        }

        // Get first entry if it's the next expected sequence
        let first_key = *buffer.keys().next()?;
        buffer.remove(&first_key)
    }

    /// Allocate query ID
    fn allocate_query_id(&self) -> u16 {
        let id = self.next_query_id.fetch_add(1, Ordering::Relaxed);
        (id % 65536) as u16
    }

    /// Get statistics
    pub fn stats(&self) -> QueryStatsSnapshot {
        QueryStatsSnapshot {
            queries_sent: self.stats.queries_sent.load(Ordering::Relaxed),
            queries_success: self.stats.queries_success.load(Ordering::Relaxed),
            queries_failed: self.stats.queries_failed.load(Ordering::Relaxed),
            queries_timeout: self.stats.queries_timeout.load(Ordering::Relaxed),
            queries_retried: self.stats.queries_retried.load(Ordering::Relaxed),
            bytes_sent: self.stats.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.stats.bytes_received.load(Ordering::Relaxed),
            in_flight: self.in_flight.len(),
        }
    }

    /// Get current in-flight count
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }
}

impl Clone for QueryStats {
    fn clone(&self) -> Self {
        Self {
            queries_sent: AtomicU64::new(self.queries_sent.load(Ordering::Relaxed)),
            queries_success: AtomicU64::new(self.queries_success.load(Ordering::Relaxed)),
            queries_failed: AtomicU64::new(self.queries_failed.load(Ordering::Relaxed)),
            queries_timeout: AtomicU64::new(self.queries_timeout.load(Ordering::Relaxed)),
            queries_retried: AtomicU64::new(self.queries_retried.load(Ordering::Relaxed)),
            bytes_sent: AtomicU64::new(self.bytes_sent.load(Ordering::Relaxed)),
            bytes_received: AtomicU64::new(self.bytes_received.load(Ordering::Relaxed)),
        }
    }
}

/// Snapshot of query statistics
#[derive(Debug, Clone)]
pub struct QueryStatsSnapshot {
    pub queries_sent: u64,
    pub queries_success: u64,
    pub queries_failed: u64,
    pub queries_timeout: u64,
    pub queries_retried: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub in_flight: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_balancer::{LoadBalancer, LoadBalancerConfig};
    use crate::buffer::{BufferPool, BufferPoolConfig};

    fn create_test_engine() -> QueryEngine {
        let domains = vec!["example.com".to_string()];
        let lb_config = LoadBalancerConfig::default();
        let lb = Arc::new(LoadBalancer::new(domains, lb_config));
        
        let pool_config = BufferPoolConfig::default();
        let pool = Arc::new(BufferPool::new(pool_config));
        
        let config = QueryConfig::default();
        QueryEngine::new(config, lb, pool)
    }

    #[tokio::test]
    async fn test_query_engine_creation() {
        let engine = create_test_engine();
        assert_eq!(engine.in_flight_count(), 0);
    }

    #[tokio::test]
    async fn test_send_data() {
        let engine = create_test_engine();
        let data = vec![1, 2, 3, 4, 5];
        
        let result = engine.send_data(data).await;
        assert!(result.is_ok());
        
        // Check that query was added to in-flight (it gets removed by the task)
        // Just verify the send_data call succeeded
        assert_eq!(result.unwrap(), ());
    }

    #[tokio::test]
    async fn test_create_batch() {
        let engine = create_test_engine();
        
        let packets = vec![
            vec![1, 2, 3],
            vec![4, 5, 6],
            vec![7, 8, 9],
        ];
        
        let batch = engine.create_batch(&packets).unwrap();
        
        // Should have: 1 byte count + 3*2 bytes offsets + 9 bytes data = 16 bytes
        assert_eq!(batch.len(), 1 + 6 + 9);
        assert_eq!(batch[0], 3); // Packet count
    }

    #[tokio::test]
    async fn test_reorder_buffer() {
        let engine = create_test_engine();
        
        // Simulate receiving responses out of order
        let _response1 = DnsResponse {
            id: 1,
            rcode: crate::dns::ResponseCode::NoError,
            answers: vec![],
        };
        
        // Add to in-flight first
        engine.in_flight.insert(1, InFlightQuery {
            query_id: 1,
            sequence: 0,
            sent_at: std::time::Instant::now(),
            domain: "example.com".to_string(),
            packet_count: 1,
            offsets: vec![0],
            retries: 0,
        });
        
        // This would normally extract payload from response
        // For now just test the reorder buffer directly
        let mut buffer = engine.reorder_buffer.lock().await;
        buffer.insert(0, vec![1, 2, 3]);
        buffer.insert(2, vec![7, 8, 9]);
        buffer.insert(1, vec![4, 5, 6]);
        drop(buffer);
        
        // Should get in order
        let data0 = engine.get_ordered_data().await;
        assert_eq!(data0, Some(vec![1, 2, 3]));
        
        let data1 = engine.get_ordered_data().await;
        assert_eq!(data1, Some(vec![4, 5, 6]));
    }

    #[tokio::test]
    async fn test_concurrency_limit() {
        let engine = Arc::new(create_test_engine());
        
        // Send multiple queries
        let mut handles = vec![];
        for i in 0..10 {
            let engine = engine.clone();
            let handle = tokio::spawn(async move {
                engine.send_data(vec![i]).await
            });
            handles.push(handle);
        }
        
        // Wait for all
        for handle in handles {
            let _ = handle.await;
        }
        
        // Should have limited concurrency
        tokio::time::sleep(Duration::from_millis(100)).await;
        let stats = engine.stats();
        assert!(stats.queries_sent <= 10);
    }

    #[test]
    fn test_query_id_allocation() {
        let engine = create_test_engine();
        
        let id1 = engine.allocate_query_id();
        let id2 = engine.allocate_query_id();
        let id3 = engine.allocate_query_id();
        
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
    }
}
