/// Batch processing module for handling multiple QUIC packets in a single DNS query
/// 
/// Format (veryslip-client compatible):
/// - 1 byte: packet count (N)
/// - N * 2 bytes: offsets for each packet (u16)
/// - Remaining bytes: concatenated packet data

use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum number of packets in a batch
pub const MAX_BATCH_SIZE: usize = 10;

/// Batch processing statistics
#[derive(Debug, Default)]
pub struct BatchStats {
    /// Number of batches processed
    pub batches_processed: AtomicU64,
    
    /// Total packets extracted from batches
    pub packets_extracted: AtomicU64,
    
    /// Number of batch parsing errors
    pub parse_errors: AtomicU64,
}

impl BatchStats {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Batch processor for splitting batched payloads
pub struct BatchProcessor {
    stats: BatchStats,
}

impl BatchProcessor {
    /// Create new batch processor
    pub fn new() -> Self {
        Self {
            stats: BatchStats::new(),
        }
    }
    
    /// Check if payload is batched (has multiple packets)
    /// Returns true if packet count > 1
    pub fn is_batched(&self, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }
        
        let packet_count = data[0] as usize;
        packet_count > 1
    }
    
    /// Split batch into individual packets
    /// 
    /// Returns: Vec of individual packet payloads
    /// 
    /// Format:
    /// - Byte 0: packet count (N)
    /// - Bytes 1..(1+N*2): offsets (u16 each)
    /// - Remaining bytes: packet data
    pub fn split_batch(&self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        // Need at least 1 byte for count
        if data.is_empty() {
            self.stats.parse_errors.fetch_add(1, Ordering::Relaxed);
            return Err("Empty batch data".to_string());
        }
        
        let packet_count = data[0] as usize;
        
        // Validate packet count
        if packet_count == 0 {
            self.stats.parse_errors.fetch_add(1, Ordering::Relaxed);
            return Err("Batch packet count is zero".to_string());
        }
        
        if packet_count > MAX_BATCH_SIZE {
            self.stats.parse_errors.fetch_add(1, Ordering::Relaxed);
            return Err(format!(
                "Batch packet count {} exceeds maximum {}",
                packet_count, MAX_BATCH_SIZE
            ));
        }
        
        // Single packet - return as-is (skip offset parsing)
        if packet_count == 1 {
            let header_size = 1 + 2; // count + 1 offset
            if data.len() < header_size {
                self.stats.parse_errors.fetch_add(1, Ordering::Relaxed);
                return Err("Batch data too short for single packet".to_string());
            }
            
            let packet_data = &data[header_size..];
            self.stats.batches_processed.fetch_add(1, Ordering::Relaxed);
            self.stats.packets_extracted.fetch_add(1, Ordering::Relaxed);
            return Ok(vec![packet_data.to_vec()]);
        }
        
        // Parse offsets
        let offsets_size = packet_count * 2;
        let header_size = 1 + offsets_size;
        
        if data.len() < header_size {
            self.stats.parse_errors.fetch_add(1, Ordering::Relaxed);
            return Err(format!(
                "Batch data too short: need {} bytes for header, got {}",
                header_size,
                data.len()
            ));
        }
        
        let mut offsets = Vec::with_capacity(packet_count);
        for i in 0..packet_count {
            let offset_pos = 1 + i * 2;
            let offset = u16::from_be_bytes([data[offset_pos], data[offset_pos + 1]]) as usize;
            offsets.push(offset);
        }
        
        // Extract packets using offsets
        let payload_start = header_size;
        let payload_data = &data[payload_start..];
        
        let mut packets = Vec::with_capacity(packet_count);
        for i in 0..packet_count {
            let start = offsets[i];
            let end = if i + 1 < packet_count {
                offsets[i + 1]
            } else {
                payload_data.len()
            };
            
            // Validate bounds
            if start > payload_data.len() || end > payload_data.len() || start > end {
                self.stats.parse_errors.fetch_add(1, Ordering::Relaxed);
                return Err(format!(
                    "Invalid packet bounds: packet {} has range {}..{} but payload is {} bytes",
                    i,
                    start,
                    end,
                    payload_data.len()
                ));
            }
            
            packets.push(payload_data[start..end].to_vec());
        }
        
        // Update statistics
        self.stats.batches_processed.fetch_add(1, Ordering::Relaxed);
        self.stats.packets_extracted.fetch_add(packet_count as u64, Ordering::Relaxed);
        
        Ok(packets)
    }
    
    /// Get batch processing statistics
    pub fn stats(&self) -> &BatchStats {
        &self.stats
    }
}

impl Default for BatchProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_is_batched_single_packet() {
        let processor = BatchProcessor::new();
        let data = vec![1, 0, 0, 1, 2, 3]; // count=1, offset=0, data=[1,2,3]
        assert!(!processor.is_batched(&data));
    }
    
    #[test]
    fn test_is_batched_multiple_packets() {
        let processor = BatchProcessor::new();
        let data = vec![2, 0, 0, 0, 3, 1, 2, 3, 4, 5, 6]; // count=2
        assert!(processor.is_batched(&data));
    }
    
    #[test]
    fn test_is_batched_empty() {
        let processor = BatchProcessor::new();
        assert!(!processor.is_batched(&[]));
    }
    
    #[test]
    fn test_split_batch_single_packet() {
        let processor = BatchProcessor::new();
        let data = vec![1, 0, 0, 1, 2, 3]; // count=1, offset=0, data=[1,2,3]
        
        let packets = processor.split_batch(&data).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0], vec![1, 2, 3]);
    }
    
    #[test]
    fn test_split_batch_two_packets() {
        let processor = BatchProcessor::new();
        // count=2, offset1=0, offset2=3, packet1=[1,2,3], packet2=[4,5,6]
        let data = vec![2, 0, 0, 0, 3, 1, 2, 3, 4, 5, 6];
        
        let packets = processor.split_batch(&data).unwrap();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0], vec![1, 2, 3]);
        assert_eq!(packets[1], vec![4, 5, 6]);
    }
    
    #[test]
    fn test_split_batch_three_packets() {
        let processor = BatchProcessor::new();
        // count=3, offsets=[0,2,5], packets=[[1,2],[3,4,5],[6,7]]
        let data = vec![3, 0, 0, 0, 2, 0, 5, 1, 2, 3, 4, 5, 6, 7];
        
        let packets = processor.split_batch(&data).unwrap();
        assert_eq!(packets.len(), 3);
        assert_eq!(packets[0], vec![1, 2]);
        assert_eq!(packets[1], vec![3, 4, 5]);
        assert_eq!(packets[2], vec![6, 7]);
    }
    
    #[test]
    fn test_split_batch_empty_data() {
        let processor = BatchProcessor::new();
        let result = processor.split_batch(&[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty batch data"));
    }
    
    #[test]
    fn test_split_batch_zero_count() {
        let processor = BatchProcessor::new();
        let data = vec![0]; // count=0
        let result = processor.split_batch(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("packet count is zero"));
    }
    
    #[test]
    fn test_split_batch_exceeds_max_size() {
        let processor = BatchProcessor::new();
        let data = vec![11]; // count=11 > MAX_BATCH_SIZE
        let result = processor.split_batch(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds maximum"));
    }
    
    #[test]
    fn test_split_batch_data_too_short() {
        let processor = BatchProcessor::new();
        let data = vec![2, 0]; // count=2 but missing offsets
        let result = processor.split_batch(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too short"));
    }
    
    #[test]
    fn test_split_batch_invalid_offset() {
        let processor = BatchProcessor::new();
        // count=2, offset1=0, offset2=100 (exceeds payload)
        let data = vec![2, 0, 0, 0, 100, 1, 2, 3];
        let result = processor.split_batch(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid packet bounds"));
    }
    
    #[test]
    fn test_batch_statistics() {
        let processor = BatchProcessor::new();
        
        // Process a batch
        let data = vec![2, 0, 0, 0, 3, 1, 2, 3, 4, 5, 6];
        let _ = processor.split_batch(&data).unwrap();
        
        let stats = processor.stats();
        assert_eq!(stats.batches_processed.load(Ordering::Relaxed), 1);
        assert_eq!(stats.packets_extracted.load(Ordering::Relaxed), 2);
        assert_eq!(stats.parse_errors.load(Ordering::Relaxed), 0);
        
        // Process an error
        let _ = processor.split_batch(&[]);
        assert_eq!(stats.parse_errors.load(Ordering::Relaxed), 1);
    }
    
    #[test]
    fn test_split_batch_variable_packet_sizes() {
        let processor = BatchProcessor::new();
        // count=3, offsets=[0,1,10], packets=[[A],[B,C,D,E,F,G,H,I,J],[K,L]]
        let data = vec![
            3, 0, 0, 0, 1, 0, 10,
            b'A',
            b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', b'J',
            b'K', b'L',
        ];
        
        let packets = processor.split_batch(&data).unwrap();
        assert_eq!(packets.len(), 3);
        assert_eq!(packets[0], vec![b'A']);
        assert_eq!(packets[1], vec![b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', b'J']);
        assert_eq!(packets[2], vec![b'K', b'L']);
    }
}
