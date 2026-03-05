use crate::{VerySlipError, Result};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(target_os = "linux")]
mod iouring;
#[cfg(target_os = "linux")]
pub use iouring::IoUringBackend;

mod tokio_backend;
pub use tokio_backend::TokioBackend;

/// Buffer for network I/O operations
/// Aligned to cache line boundary (64 bytes) for optimal CPU cache performance
#[derive(Clone)]
#[repr(align(64))]
pub struct Buffer {
    data: Vec<u8>,
    capacity: usize,
    pool_type: PoolType,
}

impl Buffer {
    fn new(capacity: usize, pool_type: PoolType) -> Self {
        Self {
            data: vec![0u8; capacity],
            capacity,
            pool_type,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.data.resize(self.capacity, 0);
    }

    pub fn resize(&mut self, new_len: usize) {
        self.data.resize(new_len.min(self.capacity), 0);
    }

    pub fn pool_type(&self) -> PoolType {
        self.pool_type
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolType {
    Send,
    Recv,
}

/// Configuration for buffer pool
#[derive(Debug, Clone)]
pub struct BufferPoolConfig {
    pub initial_size: usize,
    pub max_size: usize,
    pub buffer_capacity: usize,
}

impl Default for BufferPoolConfig {
    fn default() -> Self {
        Self {
            initial_size: 100,
            max_size: 10000,
            buffer_capacity: 1400, // Max MTU
        }
    }
}

/// Buffer pool for zero-copy operations
pub struct BufferPool {
    send_pool: Arc<Mutex<Vec<Buffer>>>,
    recv_pool: Arc<Mutex<Vec<Buffer>>>,
    config: BufferPoolConfig,
    stats: BufferPoolStats,
}

impl BufferPool {
    pub fn new(config: BufferPoolConfig) -> Self {
        let mut send_pool = Vec::with_capacity(config.initial_size);
        let mut recv_pool = Vec::with_capacity(config.initial_size);

        let stats = BufferPoolStats::default();

        // Pre-allocate initial buffers
        for _ in 0..config.initial_size {
            send_pool.push(Buffer::new(config.buffer_capacity, PoolType::Send));
            recv_pool.push(Buffer::new(config.buffer_capacity, PoolType::Recv));
        }

        // Track initial allocation
        stats.send_allocated.store(config.initial_size, Ordering::Relaxed);
        stats.recv_allocated.store(config.initial_size, Ordering::Relaxed);

        Self {
            send_pool: Arc::new(Mutex::new(send_pool)),
            recv_pool: Arc::new(Mutex::new(recv_pool)),
            config,
            stats,
        }
    }

    /// Acquire a buffer for sending
    pub fn acquire_send(&self) -> Result<Buffer> {
        let mut pool = self.send_pool.lock();
        
        if let Some(buffer) = pool.pop() {
            self.stats.send_acquired.fetch_add(1, Ordering::Relaxed);
            Ok(buffer)
        } else {
            let total = self.stats.total_allocated();
            if total < self.config.max_size {
                // Allocate new buffer
                self.stats.send_allocated.fetch_add(1, Ordering::Relaxed);
                Ok(Buffer::new(self.config.buffer_capacity, PoolType::Send))
            } else {
                Err(VerySlipError::BufferPoolExhausted)
            }
        }
    }

    /// Acquire a buffer for receiving
    pub fn acquire_recv(&self) -> Result<Buffer> {
        let mut pool = self.recv_pool.lock();
        
        if let Some(buffer) = pool.pop() {
            self.stats.recv_acquired.fetch_add(1, Ordering::Relaxed);
            Ok(buffer)
        } else {
            let total = self.stats.total_allocated();
            if total < self.config.max_size {
                // Allocate new buffer
                self.stats.recv_allocated.fetch_add(1, Ordering::Relaxed);
                Ok(Buffer::new(self.config.buffer_capacity, PoolType::Recv))
            } else {
                Err(VerySlipError::BufferPoolExhausted)
            }
        }
    }

    /// Release a buffer back to the pool
    pub fn release(&self, mut buffer: Buffer) {
        buffer.clear();
        
        match buffer.pool_type() {
            PoolType::Send => {
                let mut pool = self.send_pool.lock();
                if pool.len() < self.config.max_size / 2 {
                    pool.push(buffer);
                    self.stats.send_released.fetch_add(1, Ordering::Relaxed);
                }
            }
            PoolType::Recv => {
                let mut pool = self.recv_pool.lock();
                if pool.len() < self.config.max_size / 2 {
                    pool.push(buffer);
                    self.stats.recv_released.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// Get buffer pool statistics
    pub fn stats(&self) -> BufferPoolStats {
        self.stats.clone()
    }

    /// Get current pool sizes
    pub fn pool_sizes(&self) -> (usize, usize) {
        let send_size = self.send_pool.lock().len();
        let recv_size = self.recv_pool.lock().len();
        (send_size, recv_size)
    }
}

/// Buffer pool statistics
#[derive(Debug, Default)]
pub struct BufferPoolStats {
    pub send_allocated: AtomicUsize,
    pub recv_allocated: AtomicUsize,
    pub send_acquired: AtomicUsize,
    pub recv_acquired: AtomicUsize,
    pub send_released: AtomicUsize,
    pub recv_released: AtomicUsize,
}

impl Clone for BufferPoolStats {
    fn clone(&self) -> Self {
        Self {
            send_allocated: AtomicUsize::new(self.send_allocated.load(Ordering::Relaxed)),
            recv_allocated: AtomicUsize::new(self.recv_allocated.load(Ordering::Relaxed)),
            send_acquired: AtomicUsize::new(self.send_acquired.load(Ordering::Relaxed)),
            recv_acquired: AtomicUsize::new(self.recv_acquired.load(Ordering::Relaxed)),
            send_released: AtomicUsize::new(self.send_released.load(Ordering::Relaxed)),
            recv_released: AtomicUsize::new(self.recv_released.load(Ordering::Relaxed)),
        }
    }
}

impl BufferPoolStats {
    pub fn total_allocated(&self) -> usize {
        self.send_allocated.load(Ordering::Relaxed) + 
        self.recv_allocated.load(Ordering::Relaxed)
    }

    pub fn total_acquired(&self) -> usize {
        self.send_acquired.load(Ordering::Relaxed) + 
        self.recv_acquired.load(Ordering::Relaxed)
    }

    pub fn total_released(&self) -> usize {
        self.send_released.load(Ordering::Relaxed) + 
        self.recv_released.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_creation() {
        let buffer = Buffer::new(1400, PoolType::Send);
        assert_eq!(buffer.capacity(), 1400);
        assert_eq!(buffer.len(), 1400);
        assert_eq!(buffer.pool_type(), PoolType::Send);
    }

    #[test]
    fn test_buffer_pool_acquire_release() {
        let config = BufferPoolConfig::default();
        let pool = BufferPool::new(config);

        let buffer = pool.acquire_send().unwrap();
        assert_eq!(buffer.pool_type(), PoolType::Send);

        pool.release(buffer);

        let stats = pool.stats();
        assert_eq!(stats.send_acquired.load(Ordering::Relaxed), 1);
        assert_eq!(stats.send_released.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_buffer_pool_exhaustion() {
        let config = BufferPoolConfig {
            initial_size: 2,
            max_size: 6, // 2 send + 2 recv initial = 4, allow 2 more
            buffer_capacity: 1400,
        };
        let pool = BufferPool::new(config);

        // Acquire initial buffers (from pool)
        let mut buffers = Vec::new();
        buffers.push(pool.acquire_send().unwrap()); // from pool
        buffers.push(pool.acquire_send().unwrap()); // from pool
        
        // Acquire additional (will allocate new)
        buffers.push(pool.acquire_send().unwrap()); // allocate new (total = 5)
        buffers.push(pool.acquire_send().unwrap()); // allocate new (total = 6)

        // Should fail when exhausted
        assert!(pool.acquire_send().is_err());

        // Release one and try again
        pool.release(buffers.pop().unwrap());
        assert!(pool.acquire_send().is_ok());
    }

    #[test]
    fn test_buffer_clear() {
        let mut buffer = Buffer::new(100, PoolType::Send);
        buffer.as_mut_slice()[0] = 42;
        assert_eq!(buffer.as_slice()[0], 42);

        buffer.clear();
        assert_eq!(buffer.as_slice()[0], 0);
    }
}
