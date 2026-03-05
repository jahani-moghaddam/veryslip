// Standalone test for compression module
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Compression flags (must match veryslip-client)
pub const COMPRESSION_FLAG_COMPRESSED: u8 = 0x01;
pub const COMPRESSION_FLAG_UNCOMPRESSED: u8 = 0x00;

/// Compression statistics
#[derive(Debug, Default)]
pub struct CompressionStats {
    pub compressed_count: AtomicU64,
    pub decompressed_count: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
}

impl CompressionStats {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn compression_ratio(&self) -> f64 {
        let bytes_in = self.bytes_in.load(Ordering::Relaxed) as f64;
        let bytes_out = self.bytes_out.load(Ordering::Relaxed) as f64;
        
        if bytes_in == 0.0 {
            return 0.0;
        }
        
        1.0 - (bytes_out / bytes_in)
    }
}

pub struct CompressionEngine {
    level: i32,
    stats: Arc<CompressionStats>,
}

impl CompressionEngine {
    pub fn new(level: i32) -> Result<Self, String> {
        if !(1..=9).contains(&level) {
            return Err(format!("Invalid compression level: {} (must be 1-9)", level));
        }
        
        Ok(Self {
            level,
            stats: Arc::new(CompressionStats::new()),
        })
    }
    
    pub fn compress(&self, data: &[u8]) -> Result<(Vec<u8>, bool), String> {
        if data.is_empty() {
            let result = vec![COMPRESSION_FLAG_UNCOMPRESSED];
            return Ok((result, false));
        }
        
        match zstd::encode_all(data, self.level) {
            Ok(compressed) => {
                if compressed.len() < data.len() {
                    let mut result = Vec::with_capacity(compressed.len() + 1);
                    result.push(COMPRESSION_FLAG_COMPRESSED);
                    result.extend_from_slice(&compressed);
                    
                    self.stats.compressed_count.fetch_add(1, Ordering::Relaxed);
                    self.stats.bytes_in.fetch_add(data.len() as u64, Ordering::Relaxed);
                    self.stats.bytes_out.fetch_add(compressed.len() as u64, Ordering::Relaxed);
                    
                    Ok((result, true))
                } else {
                    let mut result = Vec::with_capacity(data.len() + 1);
                    result.push(COMPRESSION_FLAG_UNCOMPRESSED);
                    result.extend_from_slice(data);
                    Ok((result, false))
                }
            }
            Err(e) => {
                let mut result = Vec::with_capacity(data.len() + 1);
                result.push(COMPRESSION_FLAG_UNCOMPRESSED);
                result.extend_from_slice(data);
                Ok((result, false))
            }
        }
    }
    
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        if data.is_empty() {
            return Err("Empty payload".to_string());
        }
        
        let flag = data[0];
        let payload = &data[1..];
        
        match flag {
            COMPRESSION_FLAG_UNCOMPRESSED => {
                Ok(payload.to_vec())
            }
            COMPRESSION_FLAG_COMPRESSED => {
                match zstd::decode_all(payload) {
                    Ok(decompressed) => {
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
    
    pub fn stats(&self) -> Arc<CompressionStats> {
        Arc::clone(&self.stats)
    }
}

fn main() {
    println!("Testing compression module...\n");
    
    // Test 1: Engine creation
    println!("Test 1: Engine creation");
    assert!(CompressionEngine::new(5).is_ok());
    assert!(CompressionEngine::new(0).is_err());
    println!("✓ Passed\n");
    
    // Test 2: Empty payload
    println!("Test 2: Empty payload");
    let engine = CompressionEngine::new(5).unwrap();
    let (compressed, was_compressed) = engine.compress(&[]).unwrap();
    assert_eq!(compressed, vec![COMPRESSION_FLAG_UNCOMPRESSED]);
    assert!(!was_compressed);
    println!("✓ Passed\n");
    
    // Test 3: Compression round-trip
    println!("Test 3: Compression round-trip");
    let data = b"Hello, World!".repeat(100);
    let (compressed, was_compressed) = engine.compress(&data).unwrap();
    println!("  Original size: {} bytes", data.len());
    println!("  Compressed size: {} bytes", compressed.len());
    println!("  Was compressed: {}", was_compressed);
    assert!(was_compressed);
    
    let decompressed = engine.decompress(&compressed).unwrap();
    assert_eq!(data, decompressed.as_slice());
    println!("✓ Passed\n");
    
    // Test 4: Invalid flag
    println!("Test 4: Invalid compression flag");
    let payload = vec![0x99, 1, 2, 3];
    let result = engine.decompress(&payload);
    assert!(result.is_err());
    println!("✓ Passed\n");
    
    // Test 5: Statistics
    println!("Test 5: Compression statistics");
    let stats = engine.stats();
    println!("  Compressed count: {}", stats.compressed_count.load(Ordering::Relaxed));
    println!("  Decompressed count: {}", stats.decompressed_count.load(Ordering::Relaxed));
    println!("  Compression ratio: {:.2}%", stats.compression_ratio() * 100.0);
    assert!(stats.compression_ratio() > 0.0);
    println!("✓ Passed\n");
    
    println!("All tests passed! ✓");
}
