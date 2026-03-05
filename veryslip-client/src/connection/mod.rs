use crate::{VerySlipError, Result};
use crate::quic::QuicClient;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use quinn::Connection;

/// Connection pool for QUIC connections
pub struct ConnectionPool {
    connections: Arc<DashMap<ConnectionId, Arc<QuicConnection>>>,
    config: ConnectionPoolConfig,
    next_connection_id: AtomicU64,
    stats: ConnectionPoolStats,
    quic_client: Arc<QuicClient>,
    server_addr: String,
}

/// Connection pool configuration
#[derive(Debug, Clone)]
pub struct ConnectionPoolConfig {
    pub max_connections: usize,
    pub max_streams_per_conn: usize,
    pub idle_timeout: Duration,
    pub quic_config: QuicConfig,
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            max_streams_per_conn: 100,
            idle_timeout: Duration::from_secs(60),
            quic_config: QuicConfig::default(),
        }
    }
}

/// QUIC configuration for slipstream compatibility
#[derive(Debug, Clone)]
pub struct QuicConfig {
    pub max_idle_timeout: Duration,
    pub keep_alive_interval: Duration,
    pub max_concurrent_streams: u64,
    pub initial_max_data: u64,
    pub initial_max_stream_data: u64,
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            max_idle_timeout: Duration::from_secs(60),
            keep_alive_interval: Duration::from_secs(20),
            max_concurrent_streams: 100,
            initial_max_data: 8 * 1024 * 1024, // 8MB
            initial_max_stream_data: 1024 * 1024, // 1MB
        }
    }
}

/// Connection ID
pub type ConnectionId = u64;

/// Stream ID
pub type StreamId = u64;

/// QUIC connection
pub struct QuicConnection {
    pub id: ConnectionId,
    pub streams: Arc<DashMap<StreamId, Stream>>,
    pub created_at: Instant,
    pub last_used: Arc<AtomicU64>, // Unix timestamp
    pub stream_count: AtomicUsize,
    next_stream_id: AtomicU64,
    pub quinn_connection: Connection,
}

impl QuicConnection {
    fn new(id: ConnectionId, quinn_connection: Connection) -> Self {
        Self {
            id,
            streams: Arc::new(DashMap::new()),
            created_at: Instant::now(),
            last_used: Arc::new(AtomicU64::new(Self::current_timestamp())),
            stream_count: AtomicUsize::new(0),
            next_stream_id: AtomicU64::new(0),
            quinn_connection,
        }
    }

    fn allocate_stream_id(&self) -> StreamId {
        self.next_stream_id.fetch_add(1, Ordering::Relaxed)
    }

    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    pub fn update_last_used(&self) {
        self.last_used.store(Self::current_timestamp(), Ordering::Relaxed);
    }

    pub fn is_idle(&self, timeout: Duration) -> bool {
        let last_used = self.last_used.load(Ordering::Relaxed);
        let now = Self::current_timestamp();
        now - last_used > timeout.as_secs()
    }
}

/// Stream state
#[derive(Debug, Clone)]
pub struct Stream {
    pub id: StreamId,
    pub connection_id: ConnectionId,
    pub state: StreamState,
    pub created_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Open,
    Active,
    SendClosed,
    RecvClosed,
    Closed,
}

/// Connection pool statistics
#[derive(Debug, Default)]
pub struct ConnectionPoolStats {
    pub connections_created: AtomicU64,
    pub connections_closed: AtomicU64,
    pub streams_opened: AtomicU64,
    pub streams_closed: AtomicU64,
}

impl ConnectionPool {
    /// Create new connection pool
    pub fn new(config: ConnectionPoolConfig, server_addr: String) -> Result<Self> {
        let quic_client = QuicClient::new()?;
        Ok(Self {
            connections: Arc::new(DashMap::new()),
            config,
            next_connection_id: AtomicU64::new(1),
            stats: ConnectionPoolStats::default(),
            quic_client: Arc::new(quic_client),
            server_addr,
        })
    }

    /// Get or create a connection with available capacity
    pub async fn get_connection(&self) -> Result<Arc<QuicConnection>> {
        // Find connection with available capacity
        for entry in self.connections.iter() {
            let conn = entry.value();
            if conn.stream_count.load(Ordering::Relaxed) < self.config.max_streams_per_conn {
                conn.update_last_used();
                return Ok(conn.clone());
            }
        }

        // All connections at capacity, create new if under limit
        if self.connections.len() < self.config.max_connections {
            let conn_id = self.next_connection_id.fetch_add(1, Ordering::Relaxed);
            let quinn_conn = self.quic_client.connect(&self.server_addr).await?;
            let conn = Arc::new(QuicConnection::new(conn_id, quinn_conn));
            self.connections.insert(conn_id, conn.clone());
            self.stats.connections_created.fetch_add(1, Ordering::Relaxed);
            return Ok(conn);
        }

        // All connections at max capacity
        Err(VerySlipError::ConnectionClosed)
    }

    /// Open a new stream and send data
    pub async fn send_stream(&self, data: Vec<u8>) -> Result<StreamId> {
        let conn = self.get_connection().await?;
        
        let stream_id = conn.allocate_stream_id();
        let stream = Stream {
            id: stream_id,
            connection_id: conn.id,
            state: StreamState::Open,
            created_at: Instant::now(),
        };

        conn.streams.insert(stream_id, stream);
        conn.stream_count.fetch_add(1, Ordering::Relaxed);
        self.stats.streams_opened.fetch_add(1, Ordering::Relaxed);

        // Actually send data through QUIC stream
        let (mut send, _recv) = self.quic_client.open_bi(&conn.quinn_connection).await?;
        QuicClient::send_data(&mut send, &data).await?;

        Ok(stream_id)
    }

    /// Receive data from stream
    pub async fn recv_stream(&self, connection_id: ConnectionId, stream_id: StreamId) -> Result<Vec<u8>> {
        let conn = self.connections.get(&connection_id)
            .ok_or_else(|| VerySlipError::ConnectionClosed)?;
        
        // Open receive stream
        let (_send, mut recv) = self.quic_client.open_bi(&conn.quinn_connection).await?;
        let data = QuicClient::recv_data(&mut recv).await?;
        
        // Update stream state
        if let Some(mut stream) = conn.streams.get_mut(&stream_id) {
            stream.state = StreamState::RecvClosed;
        }
        
        Ok(data)
    }

    /// Close a stream
    pub async fn close_stream(&self, connection_id: ConnectionId, stream_id: StreamId) -> Result<()> {
        if let Some(conn) = self.connections.get(&connection_id) {
            if let Some((_, mut stream)) = conn.streams.remove(&stream_id) {
                stream.state = StreamState::Closed;
                conn.stream_count.fetch_sub(1, Ordering::Relaxed);
                self.stats.streams_closed.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Close idle connections
    pub async fn close_idle_connections(&self) {
        let mut to_remove = Vec::new();

        for entry in self.connections.iter() {
            let conn = entry.value();
            if conn.is_idle(self.config.idle_timeout) && conn.stream_count.load(Ordering::Relaxed) == 0 {
                to_remove.push(conn.id);
            }
        }

        for conn_id in to_remove {
            self.connections.remove(&conn_id);
            self.stats.connections_closed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Remove failed connection and redistribute streams
    pub async fn handle_connection_failure(&self, connection_id: ConnectionId) -> Result<()> {
        if let Some((_, conn)) = self.connections.remove(&connection_id) {
            self.stats.connections_closed.fetch_add(1, Ordering::Relaxed);

            // Get all active streams that need redistribution
            let streams: Vec<_> = conn.streams.iter()
                .filter(|entry| entry.value().state != StreamState::Closed)
                .map(|entry| (entry.key().clone(), entry.value().clone()))
                .collect();

            tracing::warn!(
                "Connection {} failed with {} active streams, attempting redistribution",
                connection_id,
                streams.len()
            );

            // Attempt to redistribute each stream to a new connection
            for (stream_id, stream) in streams {
                // Try to get a new connection
                match self.get_connection().await {
                    Ok(new_conn) => {
                        // Create new stream on new connection
                        let new_stream_id = new_conn.allocate_stream_id();
                        let new_stream = Stream {
                            id: new_stream_id,
                            connection_id: new_conn.id,
                            state: StreamState::Open,
                            created_at: std::time::Instant::now(),
                        };
                        
                        new_conn.streams.insert(new_stream_id, new_stream);
                        new_conn.stream_count.fetch_add(1, Ordering::Relaxed);
                        
                        tracing::info!(
                            "Redistributed stream {} from connection {} to connection {} (new stream {})",
                            stream_id,
                            connection_id,
                            new_conn.id,
                            new_stream_id
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to redistribute stream {}: {}. Stream will be closed.",
                            stream_id,
                            e
                        );
                        // Mark original stream as closed since we can't redistribute
                        self.close_stream(connection_id, stream_id).await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> ConnectionPoolStatsSnapshot {
        ConnectionPoolStatsSnapshot {
            connections_created: self.stats.connections_created.load(Ordering::Relaxed),
            connections_closed: self.stats.connections_closed.load(Ordering::Relaxed),
            connections_active: self.connections.len(),
            streams_opened: self.stats.streams_opened.load(Ordering::Relaxed),
            streams_closed: self.stats.streams_closed.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of connection pool statistics
#[derive(Debug, Clone)]
pub struct ConnectionPoolStatsSnapshot {
    pub connections_created: u64,
    pub connections_closed: u64,
    pub connections_active: usize,
    pub streams_opened: u64,
    pub streams_closed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_pool_creation() {
        let config = ConnectionPoolConfig::default();
        let pool = ConnectionPool::new(config, "127.0.0.1:4433".to_string());
        
        assert!(pool.is_ok());
        assert_eq!(pool.unwrap().connections.len(), 0);
    }

    #[test]
    fn test_stream_state() {
        let stream = Stream {
            id: 1,
            connection_id: 1,
            state: StreamState::Open,
            created_at: Instant::now(),
        };
        
        assert_eq!(stream.state, StreamState::Open);
    }
}
