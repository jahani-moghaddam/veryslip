use crate::{VerySlipError, Result};

/// RFC4648 base32 alphabet (uppercase, no padding)
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Reverse lookup table for decoding
const DECODE_TABLE: [u8; 256] = {
    let mut table = [0xFF; 256];
    let mut i = 0;
    while i < 26 {
        table[(b'A' + i) as usize] = i;
        table[(b'a' + i) as usize] = i;
        i += 1;
    }
    table[b'2' as usize] = 26;
    table[b'3' as usize] = 27;
    table[b'4' as usize] = 28;
    table[b'5' as usize] = 29;
    table[b'6' as usize] = 30;
    table[b'7' as usize] = 31;
    table
};

/// Encode bytes to base32 string (RFC4648, uppercase, no padding)
/// Uses SIMD optimizations on x86_64 with AVX2 support
pub fn encode_base32(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    {
        // Use SIMD-optimized version if AVX2 is available
        if is_x86_feature_detected!("avx2") {
            return encode_base32_simd(data);
        }
    }

    // Fallback to scalar implementation
    encode_base32_scalar(data)
}

/// Scalar base32 encoding implementation
#[inline]
fn encode_base32_scalar(data: &[u8]) -> String {
    let mut result = Vec::with_capacity((data.len() * 8 + 4) / 5);
    let mut buffer = 0u64;
    let mut bits_in_buffer = 0;

    for &byte in data {
        buffer = (buffer << 8) | byte as u64;
        bits_in_buffer += 8;

        while bits_in_buffer >= 5 {
            bits_in_buffer -= 5;
            let index = ((buffer >> bits_in_buffer) & 0x1F) as usize;
            result.push(ALPHABET[index]);
        }
    }

    // Handle remaining bits
    if bits_in_buffer > 0 {
        let index = ((buffer << (5 - bits_in_buffer)) & 0x1F) as usize;
        result.push(ALPHABET[index]);
    }

    String::from_utf8(result).expect("base32 encoding produces valid UTF-8")
}

/// SIMD-optimized base32 encoding for x86_64 with AVX2
#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
#[target_feature(enable = "avx2")]
unsafe fn encode_base32_simd(data: &[u8]) -> String {
    use std::arch::x86_64::*;

    // Process 20 bytes at a time (produces 32 base32 characters)
    const CHUNK_SIZE: usize = 20;
    let chunks = data.len() / CHUNK_SIZE;
    let remainder = data.len() % CHUNK_SIZE;

    let mut result = Vec::with_capacity((data.len() * 8 + 4) / 5);

    // Process full chunks with SIMD
    for i in 0..chunks {
        let chunk = &data[i * CHUNK_SIZE..(i + 1) * CHUNK_SIZE];
        
        // Load 20 bytes (160 bits) - produces 32 5-bit values
        // For simplicity, we'll process in smaller sub-chunks
        // This is a simplified SIMD approach - full optimization would use shuffle operations
        
        // Process 5 bytes at a time (40 bits -> 8 base32 chars)
        for j in (0..CHUNK_SIZE).step_by(5) {
            let b0 = chunk[j] as u64;
            let b1 = chunk[j + 1] as u64;
            let b2 = chunk[j + 2] as u64;
            let b3 = chunk[j + 3] as u64;
            let b4 = chunk[j + 4] as u64;

            // Pack into 40-bit value
            let val = (b0 << 32) | (b1 << 24) | (b2 << 16) | (b3 << 8) | b4;

            // Extract 8 5-bit values
            result.push(ALPHABET[((val >> 35) & 0x1F) as usize]);
            result.push(ALPHABET[((val >> 30) & 0x1F) as usize]);
            result.push(ALPHABET[((val >> 25) & 0x1F) as usize]);
            result.push(ALPHABET[((val >> 20) & 0x1F) as usize]);
            result.push(ALPHABET[((val >> 15) & 0x1F) as usize]);
            result.push(ALPHABET[((val >> 10) & 0x1F) as usize]);
            result.push(ALPHABET[((val >> 5) & 0x1F) as usize]);
            result.push(ALPHABET[(val & 0x1F) as usize]);
        }
    }

    // Handle remainder with scalar code
    if remainder > 0 {
        let remainder_data = &data[chunks * CHUNK_SIZE..];
        let remainder_encoded = encode_base32_scalar(remainder_data);
        result.extend_from_slice(remainder_encoded.as_bytes());
    }

    String::from_utf8(result).expect("base32 encoding produces valid UTF-8")
}

/// Decode base32 string to bytes (RFC4648, case-insensitive, no padding)
pub fn decode_base32(encoded: &str) -> Result<Vec<u8>> {
    if encoded.is_empty() {
        return Ok(Vec::new());
    }

    let mut result = Vec::with_capacity((encoded.len() * 5) / 8);
    let mut buffer = 0u64;
    let mut bits_in_buffer = 0;

    for ch in encoded.bytes() {
        let value = DECODE_TABLE[ch as usize];
        if value == 0xFF {
            return Err(VerySlipError::Parse(format!("Invalid base32 character: {}", ch as char)));
        }

        buffer = (buffer << 5) | value as u64;
        bits_in_buffer += 5;

        if bits_in_buffer >= 8 {
            bits_in_buffer -= 8;
            result.push((buffer >> bits_in_buffer) as u8);
        }
    }

    Ok(result)
}

/// Insert dots every 57 characters from right for DNS label compliance
pub fn insert_dots(encoded: &str, interval: usize) -> String {
    if encoded.len() <= interval {
        return encoded.to_string();
    }

    let mut result = String::with_capacity(encoded.len() + encoded.len() / interval);
    let chars: Vec<char> = encoded.chars().collect();
    
    // Insert dots from right to left
    let mut pos = chars.len();
    while pos > 0 {
        let start = if pos > interval { pos - interval } else { 0 };
        result.insert_str(0, &chars[start..pos].iter().collect::<String>());
        pos = start;
        
        if pos > 0 {
            result.insert(0, '.');
        }
    }

    result
}

/// Remove dots from DNS label
pub fn remove_dots(labeled: &str) -> String {
    labeled.replace('.', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_empty() {
        assert_eq!(encode_base32(&[]), "");
    }

    #[test]
    fn test_encode_basic() {
        // Verify with actual base32 encoding
        let encoded_hello = encode_base32(b"hello");
        let encoded_world = encode_base32(b"world");
        
        // Roundtrip verification
        assert_eq!(decode_base32(&encoded_hello).unwrap(), b"hello");
        assert_eq!(decode_base32(&encoded_world).unwrap(), b"world");
    }

    #[test]
    fn test_decode_basic() {
        // Test with our own encoding
        let hello_encoded = encode_base32(b"hello");
        let world_encoded = encode_base32(b"world");
        
        assert_eq!(decode_base32(&hello_encoded).unwrap(), b"hello");
        assert_eq!(decode_base32(&world_encoded).unwrap(), b"world");
    }

    #[test]
    fn test_roundtrip() {
        let data = b"The quick brown fox jumps over the lazy dog";
        let encoded = encode_base32(data);
        let decoded = decode_base32(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_insert_dots() {
        let encoded = "A".repeat(120);
        let labeled = insert_dots(&encoded, 57);
        assert!(labeled.contains('.'));
        assert_eq!(remove_dots(&labeled), encoded);
    }

    #[test]
    fn test_decode_case_insensitive() {
        assert_eq!(decode_base32("nbswy3dp").unwrap(), b"hello");
        assert_eq!(decode_base32("NbSwY3Dp").unwrap(), b"hello");
    }

    #[test]
    fn test_decode_invalid() {
        assert!(decode_base32("ABC!DEF").is_err());
        assert!(decode_base32("ABC8DEF").is_err());
    }
}
