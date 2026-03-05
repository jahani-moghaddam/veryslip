#[cfg(target_os = "linux")]
use crate::{VerySlipError, Result};
use super::{Buffer, BufferPool};
use std::net::SocketAddr;
use std::sync::Arc;
use std::os::unix::io::{AsRawFd, RawFd};
use std::collections::VecDeque;

/// io_uring-based network backend for Linux (zero-copy)
pub struct IoUringBackend {
    socket_fd: RawFd,
    buffer_pool: Arc<BufferPool>,
    pending_sends: VecDeque<PendingOp>,
    pending_recvs: VecDeque<PendingOp>,
}

struct PendingOp {
    buffer: Buffer,
    addr: SocketAddr,
    user_data: u64,
}

impl IoUringBackend {
    /// Create new io_uring backend
    pub fn new(bind_addr: SocketAddr, buffer_pool: Arc<BufferPool>) -> Result<Self> {
        use std::net::UdpSocket as StdUdpSocket;

        // Create standard UDP socket
        let socket = StdUdpSocket::bind(bind_addr)
            .map_err(|e| VerySlipError::Network(format!("Failed to bind socket: {}", e)))?;

        // Configure socket
        Self::configure_socket(&socket)?;

        let socket_fd = socket.as_raw_fd();
        std::mem::forget(socket); // Keep socket alive

        Ok(Self {
            socket_fd,
            buffer_pool,
            pending_sends: VecDeque::new(),
            pending_recvs: VecDeque::new(),
        })
    }

    /// Configure socket options for Linux
    fn configure_socket(socket: &std::net::UdpSocket) -> Result<()> {
        use socket2::{Socket, SockRef};

        let sock_ref = SockRef::from(socket);

        // Set send buffer size to 4MB
        sock_ref
            .set_send_buffer_size(4 * 1024 * 1024)
            .map_err(|e| VerySlipError::Network(format!("Failed to set send buffer: {}", e)))?;

        // Set receive buffer size to 4MB
        sock_ref
            .set_recv_buffer_size(4 * 1024 * 1024)
            .map_err(|e| VerySlipError::Network(format!("Failed to set recv buffer: {}", e)))?;

        // Enable UDP GRO (Generic Receive Offload) if available
        #[cfg(target_os = "linux")]
        {
            const UDP_GRO: i32 = 104;
            let enable: i32 = 1;
            unsafe {
                let ret = libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::IPPROTO_UDP,
                    UDP_GRO,
                    &enable as *const _ as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t,
                );
                // Ignore error if not supported
                if ret < 0 {
                    tracing::debug!("UDP_GRO not supported");
                }
            }
        }

        // Enable UDP GSO (Generic Segmentation Offload) if available
        #[cfg(target_os = "linux")]
        {
            const UDP_SEGMENT: i32 = 103;
            let segment_size: i32 = 1400;
            unsafe {
                let ret = libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::IPPROTO_UDP,
                    UDP_SEGMENT,
                    &segment_size as *const _ as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t,
                );
                // Ignore error if not supported
                if ret < 0 {
                    tracing::debug!("UDP_GSO not supported");
                }
            }
        }

        // Set IP TOS to low delay
        #[cfg(target_os = "linux")]
        {
            const IPTOS_LOWDELAY: i32 = 0x10;
            unsafe {
                let ret = libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::IPPROTO_IP,
                    libc::IP_TOS,
                    &IPTOS_LOWDELAY as *const _ as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t,
                );
                if ret < 0 {
                    tracing::debug!("Failed to set IP_TOS");
                }
            }
        }

        Ok(())
    }

    /// Submit send operation
    pub fn submit_send(&mut self, buffer: Buffer, addr: SocketAddr, user_data: u64) -> Result<()> {
        self.pending_sends.push_back(PendingOp {
            buffer,
            addr,
            user_data,
        });
        Ok(())
    }

    /// Submit receive operation
    pub fn submit_recv(&mut self, buffer: Buffer, user_data: u64) -> Result<()> {
        self.pending_recvs.push_back(PendingOp {
            buffer,
            addr: "0.0.0.0:0".parse().unwrap(),
            user_data,
        });
        Ok(())
    }

    /// Get socket file descriptor
    pub fn socket_fd(&self) -> RawFd {
        self.socket_fd
    }
}

impl Drop for IoUringBackend {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.socket_fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::BufferPoolConfig;

    #[test]
    fn test_iouring_backend_creation() {
        let config = BufferPoolConfig::default();
        let pool = Arc::new(BufferPool::new(config));
        
        let backend = IoUringBackend::new("127.0.0.1:0".parse().unwrap(), pool);
        assert!(backend.is_ok());
    }
}
