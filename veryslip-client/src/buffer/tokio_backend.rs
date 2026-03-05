use crate::{VerySlipError, Result};
use super::{Buffer, BufferPool};
use tokio::net::UdpSocket;
use std::net::SocketAddr;
use std::sync::Arc;

/// Tokio-based network backend (fallback for non-Linux platforms)
pub struct TokioBackend {
    socket: Arc<UdpSocket>,
    buffer_pool: Arc<BufferPool>,
}

impl TokioBackend {
    /// Create new tokio backend
    pub async fn new(bind_addr: SocketAddr, buffer_pool: Arc<BufferPool>) -> Result<Self> {
        let socket = UdpSocket::bind(bind_addr)
            .await
            .map_err(|e| VerySlipError::Network(format!("Failed to bind socket: {}", e)))?;

        // Configure socket buffers
        Self::configure_socket(&socket)?;

        Ok(Self {
            socket: Arc::new(socket),
            buffer_pool,
        })
    }

    /// Configure socket options
    fn configure_socket(socket: &UdpSocket) -> Result<()> {
        use socket2::SockRef;

        let sock_ref = SockRef::from(socket);

        // Set send buffer size to 4MB
        sock_ref
            .set_send_buffer_size(4 * 1024 * 1024)
            .map_err(|e| VerySlipError::Network(format!("Failed to set send buffer: {}", e)))?;

        // Set receive buffer size to 4MB
        sock_ref
            .set_recv_buffer_size(4 * 1024 * 1024)
            .map_err(|e| VerySlipError::Network(format!("Failed to set recv buffer: {}", e)))?;

        Ok(())
    }

    /// Send data to address
    pub async fn send_to(&self, data: &[u8], addr: SocketAddr) -> Result<usize> {
        self.socket
            .send_to(data, addr)
            .await
            .map_err(|e| VerySlipError::Network(format!("Send failed: {}", e)))
    }

    /// Receive data from socket
    pub async fn recv_from(&self) -> Result<(Buffer, usize, SocketAddr)> {
        let mut buffer = self.buffer_pool.acquire_recv()?;
        
        let (len, addr) = self.socket
            .recv_from(buffer.as_mut_slice())
            .await
            .map_err(|e| VerySlipError::Network(format!("Recv failed: {}", e)))?;

        buffer.resize(len);
        Ok((buffer, len, addr))
    }

    /// Get local address
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket
            .local_addr()
            .map_err(|e| VerySlipError::Network(format!("Failed to get local addr: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::BufferPoolConfig;

    #[tokio::test]
    async fn test_tokio_backend_creation() {
        let config = BufferPoolConfig::default();
        let pool = Arc::new(BufferPool::new(config));
        
        let backend = TokioBackend::new("127.0.0.1:0".parse().unwrap(), pool).await;
        assert!(backend.is_ok());
    }

    #[tokio::test]
    async fn test_send_recv() {
        let config = BufferPoolConfig::default();
        let pool = Arc::new(BufferPool::new(config));
        
        let backend1 = TokioBackend::new("127.0.0.1:0".parse().unwrap(), pool.clone())
            .await
            .unwrap();
        let addr1 = backend1.local_addr().unwrap();

        let backend2 = TokioBackend::new("127.0.0.1:0".parse().unwrap(), pool.clone())
            .await
            .unwrap();
        let addr2 = backend2.local_addr().unwrap();

        // Send from backend1 to backend2
        let data = b"hello world";
        backend1.send_to(data, addr2).await.unwrap();

        // Receive on backend2
        let (buffer, len, from_addr) = backend2.recv_from().await.unwrap();
        assert_eq!(len, data.len());
        assert_eq!(&buffer.as_slice()[..len], data);
        assert_eq!(from_addr, addr1);
    }
}
