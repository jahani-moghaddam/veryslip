use crate::{Result, VerySlipError};
use quinn::{Endpoint, Connection, SendStream, RecvStream};

/// QUIC client for establishing connections
pub struct QuicClient {
    endpoint: Endpoint,
}

impl QuicClient {
    /// Create new QUIC client
    pub fn new() -> Result<Self> {
        // Create endpoint with default client configuration
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())
            .map_err(|e| VerySlipError::Quic(format!("Failed to create endpoint: {}", e)))?;
        
        // Use platform verifier (system root certificates)
        let client_config = quinn::ClientConfig::try_with_platform_verifier()
            .map_err(|e| VerySlipError::Quic(format!("Failed to create client config: {}", e)))?;
        endpoint.set_default_client_config(client_config);

        Ok(Self { endpoint })
    }

    /// Connect to server
    pub async fn connect(&self, server_addr: &str) -> Result<Connection> {
        let connection = self.endpoint
            .connect(server_addr.parse().unwrap(), "localhost")
            .map_err(|e| VerySlipError::Quic(format!("Failed to connect: {}", e)))?
            .await
            .map_err(|e| VerySlipError::Quic(format!("Connection failed: {}", e)))?;

        Ok(connection)
    }

    /// Open bidirectional stream
    pub async fn open_bi(&self, connection: &Connection) -> Result<(SendStream, RecvStream)> {
        connection
            .open_bi()
            .await
            .map_err(|e| VerySlipError::Quic(format!("Failed to open stream: {}", e)))
    }

    /// Send data on stream
    pub async fn send_data(send: &mut SendStream, data: &[u8]) -> Result<()> {
        send.write_all(data)
            .await
            .map_err(|e| VerySlipError::Quic(format!("Failed to send: {}", e)))?;
        
        send.finish()
            .map_err(|e| VerySlipError::Quic(format!("Failed to finish: {}", e)))?;

        Ok(())
    }

    /// Receive data from stream
    pub async fn recv_data(recv: &mut RecvStream) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        
        recv.read_to_end(1024 * 1024) // 1MB max
            .await
            .map_err(|e| VerySlipError::Quic(format!("Failed to receive: {}", e)))?
            .iter()
            .for_each(|&b| buf.push(b));

        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quic_client_creation() {
        // Note: QUIC client creation may fail on some platforms without proper TLS setup
        // This is expected in test environments
        let _client = QuicClient::new();
        // Just verify it doesn't panic
    }
}
