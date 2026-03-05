use crate::{VerySlipError, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Compression engine using Zstandard
pub struct CompressionEngine {
    config: CompressionConfig,
    #[allow(dead_code)]
    dictionary: Option<Arc<Vec<u8>>>,
    stats: CompressionStats,
}

/// Compression configuration
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    pub level: i32,
    pub dictionary_path: Option<String>,
    pub adaptive: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            level: 3,
            dictionary_path: None,
            adaptive: true,
        }
    }
}

impl CompressionEngine {
    /// Create new compression engine
    pub fn new(config: CompressionConfig) -> Result<Self> {
        let dictionary = if let Some(path) = &config.dictionary_path {
            let dict_data = std::fs::read(path)
                .map_err(|e| VerySlipError::Compression(format!("Failed to load dictionary: {}", e)))?;
            Some(Arc::new(dict_data))
        } else {
            None
        };

        Ok(Self {
            config,
            dictionary,
            stats: CompressionStats::default(),
        })
    }

    /// Compress data with optional content type
    pub fn compress(&self, data: &[u8], content_type: Option<&str>) -> Result<Vec<u8>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        // Check if we should compress this content
        if !self.should_compress(content_type) {
            self.stats.skipped.fetch_add(1, Ordering::Relaxed);
            return Ok(data.to_vec());
        }

        // Determine compression level
        let level = if self.config.adaptive {
            self.adaptive_level(content_type)
        } else {
            self.config.level
        };

        // Compress with zstd
        let compressed = zstd::bulk::compress(data, level)
            .map_err(|e| VerySlipError::Compression(format!("Compression failed: {}", e)))?;

        // Only use compressed if it's smaller
        if compressed.len() < data.len() {
            self.stats.compressed.fetch_add(1, Ordering::Relaxed);
            self.stats.bytes_in.fetch_add(data.len() as u64, Ordering::Relaxed);
            self.stats.bytes_out.fetch_add(compressed.len() as u64, Ordering::Relaxed);
            Ok(compressed)
        } else {
            self.stats.skipped.fetch_add(1, Ordering::Relaxed);
            Ok(data.to_vec())
        }
    }

    /// Decompress data
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        let decompressed = zstd::bulk::decompress(data, 10 * 1024 * 1024) // 10MB max
            .map_err(|e| VerySlipError::Compression(format!("Decompression failed: {}", e)))?;

        self.stats.decompressed.fetch_add(1, Ordering::Relaxed);
        Ok(decompressed)
    }

    /// Check if content should be compressed
    pub fn should_compress(&self, content_type: Option<&str>) -> bool {
        let ct = match content_type {
            Some(ct) => ct.to_lowercase(),
            None => return true, // Compress unknown types
        };

        // Skip already-compressed formats
        if ct.contains("image/jpeg") 
            || ct.contains("image/jpg")
            || ct.contains("image/png")
            || ct.contains("image/gif")
            || ct.contains("image/webp")
            || ct.contains("video/")
            || ct.contains("audio/")
            || ct.contains("application/zip")
            || ct.contains("application/gzip")
            || ct.contains("application/x-gzip")
            || ct.contains("application/x-bzip2")
            || ct.contains("application/x-xz")
        {
            return false;
        }

        true
    }

    /// Detect content type from data
    pub fn detect_content_type(&self, data: &[u8]) -> Option<String> {
        if data.len() < 8 {
            return None;
        }

        // Check magic numbers
        if data.starts_with(b"\x89PNG") {
            return Some("image/png".to_string());
        }
        if data.starts_with(b"\xFF\xD8\xFF") {
            return Some("image/jpeg".to_string());
        }
        if data.starts_with(b"GIF89a") || data.starts_with(b"GIF87a") {
            return Some("image/gif".to_string());
        }
        if data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"WEBP" {
            return Some("image/webp".to_string());
        }
        if data.len() >= 12 && &data[4..12] == b"ftypmp4" {
            return Some("video/mp4".to_string());
        }
        if data.starts_with(b"<!DOCTYPE") || data.starts_with(b"<html") || data.starts_with(b"<HTML") {
            return Some("text/html".to_string());
        }
        if data.starts_with(b"{") || data.starts_with(b"[") {
            return Some("application/json".to_string());
        }
        if data.starts_with(b"<?xml") {
            return Some("application/xml".to_string());
        }

        None
    }

    /// Get adaptive compression level based on content type
    fn adaptive_level(&self, content_type: Option<&str>) -> i32 {
        let ct = match content_type {
            Some(ct) => ct.to_lowercase(),
            None => return self.config.level,
        };

        if ct.contains("text/html") 
            || ct.contains("text/css")
            || ct.contains("application/javascript")
            || ct.contains("text/javascript")
        {
            5 // High compression for HTML/CSS/JS
        } else if ct.contains("application/json") || ct.contains("application/xml") {
            4 // Medium-high for JSON/XML
        } else if ct.contains("text/") {
            3 // Medium for plain text
        } else {
            self.config.level // Default
        }
    }

    /// Get compression statistics
    pub fn stats(&self) -> CompressionStats {
        self.stats.clone()
    }

    /// Get compression ratio
    pub fn compression_ratio(&self) -> f64 {
        let bytes_in = self.stats.bytes_in.load(Ordering::Relaxed);
        let bytes_out = self.stats.bytes_out.load(Ordering::Relaxed);
        
        if bytes_in == 0 {
            return 0.0;
        }

        1.0 - (bytes_out as f64 / bytes_in as f64)
    }
}

/// Compression statistics
#[derive(Debug, Default)]
pub struct CompressionStats {
    pub compressed: AtomicU64,
    pub decompressed: AtomicU64,
    pub skipped: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
}

impl Clone for CompressionStats {
    fn clone(&self) -> Self {
        Self {
            compressed: AtomicU64::new(self.compressed.load(Ordering::Relaxed)),
            decompressed: AtomicU64::new(self.decompressed.load(Ordering::Relaxed)),
            skipped: AtomicU64::new(self.skipped.load(Ordering::Relaxed)),
            bytes_in: AtomicU64::new(self.bytes_in.load(Ordering::Relaxed)),
            bytes_out: AtomicU64::new(self.bytes_out.load(Ordering::Relaxed)),
        }
    }
}

/// Compression flags for DNS payload
pub const COMPRESSION_FLAG_COMPRESSED: u8 = 0x01;
pub const COMPRESSION_FLAG_UNCOMPRESSED: u8 = 0x00;

/// Add compression flag to payload
pub fn add_compression_flag(data: Vec<u8>, compressed: bool) -> Vec<u8> {
    let flag = if compressed {
        COMPRESSION_FLAG_COMPRESSED
    } else {
        COMPRESSION_FLAG_UNCOMPRESSED
    };
    
    let mut result = Vec::with_capacity(data.len() + 1);
    result.push(flag);
    result.extend_from_slice(&data);
    result
}

/// Extract compression flag from payload
pub fn extract_compression_flag(data: &[u8]) -> Result<(bool, &[u8])> {
    if data.is_empty() {
        return Err(VerySlipError::Compression("Empty payload".to_string()));
    }

    let compressed = data[0] == COMPRESSION_FLAG_COMPRESSED;
    Ok((compressed, &data[1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress() {
        let config = CompressionConfig::default();
        let engine = CompressionEngine::new(config).unwrap();

        let data = b"Hello, world! This is a test string that should compress well.".repeat(10);
        let compressed = engine.compress(&data, Some("text/plain")).unwrap();
        
        assert!(compressed.len() < data.len());

        let decompressed = engine.decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_should_compress() {
        let config = CompressionConfig::default();
        let engine = CompressionEngine::new(config).unwrap();

        assert!(engine.should_compress(Some("text/html")));
        assert!(engine.should_compress(Some("application/json")));
        assert!(!engine.should_compress(Some("image/jpeg")));
        assert!(!engine.should_compress(Some("image/png")));
        assert!(!engine.should_compress(Some("video/mp4")));
    }

    #[test]
    fn test_detect_content_type() {
        let config = CompressionConfig::default();
        let engine = CompressionEngine::new(config).unwrap();

        assert_eq!(engine.detect_content_type(b"\x89PNG\r\n\x1a\n"), Some("image/png".to_string()));
        assert_eq!(engine.detect_content_type(b"\xFF\xD8\xFF\xE0\x00\x10JFIF"), Some("image/jpeg".to_string()));
        assert_eq!(engine.detect_content_type(b"<!DOCTYPE html>"), Some("text/html".to_string()));
        assert_eq!(engine.detect_content_type(b"{\"key\": \"value\"}"), Some("application/json".to_string()));
    }

    #[test]
    fn test_adaptive_level() {
        let config = CompressionConfig {
            level: 3,
            adaptive: true,
            dictionary_path: None,
        };
        let engine = CompressionEngine::new(config).unwrap();

        assert_eq!(engine.adaptive_level(Some("text/html")), 5);
        assert_eq!(engine.adaptive_level(Some("application/json")), 4);
        assert_eq!(engine.adaptive_level(Some("text/plain")), 3);
    }

    #[test]
    fn test_compression_flags() {
        let data = vec![1, 2, 3, 4, 5];
        
        let with_flag = add_compression_flag(data.clone(), true);
        assert_eq!(with_flag[0], COMPRESSION_FLAG_COMPRESSED);
        
        let (compressed, payload) = extract_compression_flag(&with_flag).unwrap();
        assert!(compressed);
        assert_eq!(payload, &data[..]);
    }

    #[test]
    fn test_compression_ratio() {
        let config = CompressionConfig::default();
        let engine = CompressionEngine::new(config).unwrap();

        let data = b"test data ".repeat(100);
        engine.compress(&data, Some("text/plain")).unwrap();

        let ratio = engine.compression_ratio();
        assert!(ratio > 0.5); // Should achieve >50% compression
    }

    #[test]
    fn test_skip_already_compressed() {
        let config = CompressionConfig::default();
        let engine = CompressionEngine::new(config).unwrap();

        let data = vec![1, 2, 3, 4, 5];
        let result = engine.compress(&data, Some("image/jpeg")).unwrap();
        
        // Should return original data unchanged
        assert_eq!(result, data);
        assert_eq!(engine.stats().skipped.load(Ordering::Relaxed), 1);
    }
}
