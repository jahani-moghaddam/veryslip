use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Compression flags (must match veryslip-client)
pub const COMPRESSION_FLAG_COMPRESSED: u8 = 0x01;
pub const COMPRESSION_FLAG_UNCOMPRESSED: u8 = 0x00;

/// Compression statistics
#[derive(Debug, Default)]
pub struct CompressionStats {
    /// Number of payloads compressed
    pub compressed_count: AtomicU64,
    
    /// Number of payloads decompressed
    pub decompressed_count: AtomicU64,
    
    /// Total bytes before compression
    pub bytes_in: AtomicU64,
    
    /// Total bytes after compression
    pub bytes_out: AtomicU64,
}

impl CompressionStats {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Get compression ratio (1.0 - bytes_out/bytes_in)
    pub fn compression_ratio(&self) -> f64 {
        let bytes_in = self.bytes_in.load(Ordering::Relaxed) as f64;
        let bytes_out = self.bytes_out.load(Ordering::Relaxed) as f64;
        
        if bytes_in == 0.0 {
            return 0.0;
        }
        
        1.0 - (bytes_out / bytes_in)
    }
}

/// Compression engine for QUIC packet payloads
pub struct CompressionEngine {
    level: i32,
    stats: Arc<CompressionStats>,
}

impl CompressionEngine {
    /// Create new compression engine with specified level (1-9)
    pub fn new(level: i32) -> Result<Self, String> {
        if !(1..=9).contains(&level) {
            return Err(format!("Invalid compression level: {} (must be 1-9)", level));
        }
        
        Ok(Self {
            level,
            stats: Arc::new(CompressionStats::new()),
        })
    }
    
    /// Compress data and add compression flag
    /// Returns: (compressed_data_with_flag, was_compressed)
    pub fn compress(&self, data: &[u8]) -> Result<(Vec<u8>, bool), String> {
        // Empty payload - return as-is with uncompressed flag
        if data.is_empty() {
            let mut result = vec![COMPRESSION_FLAG_UNCOMPRESSED];
            return Ok((result, false));
        }
        
        // Try to compress
        match zstd::encode_all(data, self.level) {
            Ok(compressed) => {
                // Only use compression if it actually reduces size
                if compressed.len() < data.len() {
                    let mut result = Vec::with_capacity(compressed.len() + 1);
                    result.push(COMPRESSION_FLAG_COMPRESSED);
                    result.extend_from_slice(&compressed);
                    
                    // Update statistics
                    self.stats.compressed_count.fetch_add(1, Ordering::Relaxed);
                    self.stats.bytes_in.fetch_add(data.len() as u64, Ordering::Relaxed);
                    self.stats.bytes_out.fetch_add(compressed.len() as u64, Ordering::Relaxed);
                    
                    Ok((result, true))
                } else {
                    // Compression didn't help, send uncompressed
                    let mut result = Vec::with_capacity(data.len() + 1);
                    result.push(COMPRESSION_FLAG_UNCOMPRESSED);
                    result.extend_from_slice(data);
                    Ok((result, false))
                }
            }
            Err(e) => {
                // Compression failed, send uncompressed
                tracing::warn!("Compression failed: {}, sending uncompressed", e);
                let mut result = Vec::with_capacity(data.len() + 1);
                result.push(COMPRESSION_FLAG_UNCOMPRESSED);
                result.extend_from_slice(data);
                Ok((result, false))
            }
        }
    }
    
    /// Decompress data after extracting compression flag
    /// Returns: decompressed_data
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        // Need at least flag byte
        if data.is_empty() {
            return Err("Empty payload".to_string());
        }
        
        let flag = data[0];
        let payload = &data[1..];
        
        match flag {
            COMPRESSION_FLAG_UNCOMPRESSED => {
                // Not compressed, return as-is
                Ok(payload.to_vec())
            }
            COMPRESSION_FLAG_COMPRESSED => {
                // Decompress
                match zstd::decode_all(payload) {
                    Ok(decompressed) => {
                        // Update statistics
                        self.stats.decompressed_count.fetch_add(1, Ordering::Relaxed);
                        Ok(decompressed)
                    }
                    Err(e) => {
                        Err(format!("Decompression failed: {}", e))
                    }
                }
            }
            _ => {
                Err(format!("Invalid compression flag: 0x{:02X}", flag))
            }
        }
    }
    
    /// Get compression statistics
    pub fn stats(&self) -> Arc<CompressionStats> {
        Arc::clone(&self.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_compression_engine_creation() {
        // Valid levels
        assert!(CompressionEngine::new(1).is_ok());
        assert!(CompressionEngine::new(5).is_ok());
        assert!(CompressionEngine::new(9).is_ok());
        
        // Invalid levels
        assert!(CompressionEngine::new(0).is_err());
        assert!(CompressionEngine::new(10).is_err());
        assert!(CompressionEngine::new(-1).is_err());
    }
    
    #[test]
    fn test_empty_payload_compression() {
        let engine = CompressionEngine::new(5).unwrap();
        let (compressed, was_compressed) = engine.compress(&[]).unwrap();
        
        assert_eq!(compressed, vec![COMPRESSION_FLAG_UNCOMPRESSED]);
        assert!(!was_compressed);
    }
    
    #[test]
    fn test_compression_round_trip() {
        let engine = CompressionEngine::new(5).unwrap();
        let data = b"Hello, World!".repeat(100);
        
        let (compressed, was_compressed) = engine.compress(&data).unwrap();
        assert!(was_compressed, "Data should be compressed");
        assert!(compressed.len() < data.len() + 1, "Compressed should be smaller");
        
        let decompressed = engine.decompress(&compressed).unwrap();
        assert_eq!(data, decompressed.as_slice());
    }
    
    #[test]
    fn test_uncompressed_round_trip() {
        let engine = CompressionEngine::new(5).unwrap();
        let data = b"abc"; // Too small to compress effectively
        
        let (with_flag, was_compressed) = engine.compress(&data).unwrap();
        let decompressed = engine.decompress(&with_flag).unwrap();
        
        assert_eq!(data, decompressed.as_slice());
    }
    
    #[test]
    fn test_invalid_compression_flag() {
        let engine = CompressionEngine::new(5).unwrap();
        let payload = vec![0x99, 1, 2, 3]; // Invalid flag
        
        let result = engine.decompress(&payload);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid compression flag"));
    }
    
    #[test]
    fn test_decompression_of_corrupted_data() {
        let engine = CompressionEngine::new(5).unwrap();
        let mut payload = vec![COMPRESSION_FLAG_COMPRESSED];
        payload.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]); // Corrupted data
        
        let result = engine.decompress(&payload);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Decompression failed"));
    }
    
    #[test]
    fn test_compression_statistics() {
        let engine = CompressionEngine::new(5).unwrap();
        let data = b"Hello, World!".repeat(100);
        
        let (compressed, _) = engine.compress(&data).unwrap();
        let _ = engine.decompress(&compressed).unwrap();
        
        let stats = engine.stats();
        assert_eq!(stats.compressed_count.load(Ordering::Relaxed), 1);
        assert_eq!(stats.decompressed_count.load(Ordering::Relaxed), 1);
        assert!(stats.bytes_in.load(Ordering::Relaxed) > 0);
        assert!(stats.bytes_out.load(Ordering::Relaxed) > 0);
        
        let ratio = stats.compression_ratio();
        assert!(ratio > 0.0 && ratio < 1.0, "Compression ratio should be between 0 and 1");
    }
    
    #[test]
    fn test_compression_ratio_with_zero_bytes() {
        let stats = CompressionStats::new();
        assert_eq!(stats.compression_ratio(), 0.0);
    }
}
