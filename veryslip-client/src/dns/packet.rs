use crate::{VerySlipError, Result};
use super::{RecordType, Class, ResponseCode, encode_base32, insert_dots, DOT_INTERVAL};
use bytes::{Buf, BufMut, BytesMut};

/// DNS query packet
#[derive(Debug, Clone)]
pub struct DnsQuery {
    pub id: u16,
    pub qname: String,
    pub qtype: RecordType,
    pub qclass: Class,
    pub payload: Vec<u8>,
}

impl DnsQuery {
    /// Create new DNS query with payload
    pub fn new(id: u16, domain: &str, payload: Vec<u8>) -> Self {
        let encoded = encode_base32(&payload);
        let labeled = insert_dots(&encoded, DOT_INTERVAL);
        let qname = format!("{}.{}", labeled, domain);

        Self {
            id,
            qname,
            qtype: RecordType::TXT,
            qclass: Class::IN,
            payload,
        }
    }

    /// Encode DNS query to wire format (RFC 1035)
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut buf = BytesMut::with_capacity(512);

        // Header (12 bytes)
        buf.put_u16(self.id);
        buf.put_u16(0x0100); // Flags: RD=1 (recursion desired)
        buf.put_u16(1); // QDCOUNT: 1 question
        buf.put_u16(0); // ANCOUNT: 0 answers
        buf.put_u16(0); // NSCOUNT: 0 authority
        buf.put_u16(1); // ARCOUNT: 1 additional (EDNS0)

        // Question section
        encode_domain_name(&mut buf, &self.qname)?;
        buf.put_u16(self.qtype as u16);
        buf.put_u16(self.qclass as u16);

        // Additional section: EDNS0 OPT record
        buf.put_u8(0); // Root domain (empty name)
        buf.put_u16(RecordType::OPT as u16);
        buf.put_u16(1232); // UDP payload size
        buf.put_u8(0); // Extended RCODE
        buf.put_u8(0); // Version
        buf.put_u16(0); // Flags
        buf.put_u16(0); // RDLENGTH (no options)

        Ok(buf.to_vec())
    }

    /// Get maximum payload size for given domain and MTU
    pub fn max_payload_size(domain: &str, mtu: usize) -> usize {
        // DNS overhead: 12 (header) + 4 (qtype+qclass) + 11 (EDNS0) = 27 bytes
        // Domain encoding: each label has 1-byte length prefix
        let domain_overhead = domain.len() + domain.split('.').count() + 1;
        let available = mtu.saturating_sub(27 + domain_overhead);
        
        // Base32 expansion: 5 bytes → 8 characters
        // With dots every 57 chars: ~2% overhead
        // Total: 1.6x expansion
        (available as f32 / 1.6) as usize
    }
}

/// DNS response packet
#[derive(Debug, Clone)]
pub struct DnsResponse {
    pub id: u16,
    pub rcode: ResponseCode,
    pub answers: Vec<DnsAnswer>,
}

impl DnsResponse {
    /// Parse DNS response from wire format
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(VerySlipError::Parse("DNS response too short".to_string()));
        }

        let mut buf = &data[..];

        // Parse header
        let id = buf.get_u16();
        let flags = buf.get_u16();
        let qdcount = buf.get_u16();
        let ancount = buf.get_u16();
        let _nscount = buf.get_u16();
        let _arcount = buf.get_u16();

        let rcode = ResponseCode::from_u8((flags & 0x0F) as u8);

        // Skip question section
        for _ in 0..qdcount {
            skip_domain_name(&mut buf)?;
            if buf.remaining() < 4 {
                return Err(VerySlipError::Parse("Truncated question section".to_string()));
            }
            buf.advance(4); // qtype + qclass
        }

        // Parse answer section
        let mut answers = Vec::with_capacity(ancount as usize);
        for _ in 0..ancount {
            answers.push(DnsAnswer::parse(&mut buf)?);
        }

        Ok(Self { id, rcode, answers })
    }

    /// Extract payload from TXT records
    pub fn extract_payload(&self) -> Result<Vec<u8>> {
        for answer in &self.answers {
            if answer.rtype == RecordType::TXT as u16 {
                return Ok(answer.data.clone());
            }
        }
        Err(VerySlipError::Dns("No TXT record in response".to_string()))
    }
}

/// DNS answer record
#[derive(Debug, Clone)]
pub struct DnsAnswer {
    pub name: String,
    pub rtype: u16,
    pub rclass: u16,
    pub ttl: u32,
    pub data: Vec<u8>,
}

impl DnsAnswer {
    /// Parse DNS answer from buffer
    fn parse(buf: &mut &[u8]) -> Result<Self> {
        let name = parse_domain_name(buf)?;
        
        if buf.remaining() < 10 {
            return Err(VerySlipError::Parse("Truncated answer record".to_string()));
        }

        let rtype = buf.get_u16();
        let rclass = buf.get_u16();
        let ttl = buf.get_u32();
        let rdlength = buf.get_u16() as usize;

        if buf.remaining() < rdlength {
            return Err(VerySlipError::Parse("Truncated answer data".to_string()));
        }

        let mut data = vec![0u8; rdlength];
        buf.copy_to_slice(&mut data);

        // For TXT records, skip length byte
        if rtype == RecordType::TXT as u16 && !data.is_empty() {
            data = data[1..].to_vec();
        }

        Ok(Self {
            name,
            rtype,
            rclass,
            ttl,
            data,
        })
    }
}

/// Encode domain name in DNS format (length-prefixed labels)
fn encode_domain_name(buf: &mut BytesMut, name: &str) -> Result<()> {
    for label in name.split('.') {
        if label.is_empty() {
            continue;
        }
        if label.len() > 63 {
            return Err(VerySlipError::Dns(format!("Label too long: {}", label.len())));
        }
        buf.put_u8(label.len() as u8);
        buf.put_slice(label.as_bytes());
    }
    buf.put_u8(0); // Root label
    Ok(())
}

/// Parse domain name from DNS format (simplified, no compression support)
fn parse_domain_name(buf: &mut &[u8]) -> Result<String> {
    let mut labels = Vec::new();

    loop {
        if buf.is_empty() {
            return Err(VerySlipError::Parse("Unexpected end of domain name".to_string()));
        }

        let len = buf[0];

        // Check for compression pointer (skip for now)
        if len & 0xC0 == 0xC0 {
            if buf.remaining() < 2 {
                return Err(VerySlipError::Parse("Truncated compression pointer".to_string()));
            }
            buf.advance(2);
            // Compression not fully supported, return partial name
            break;
        }

        buf.advance(1);

        if len == 0 {
            break;
        }

        if buf.remaining() < len as usize {
            return Err(VerySlipError::Parse("Truncated domain label".to_string()));
        }

        let label = String::from_utf8_lossy(&buf[..len as usize]).to_string();
        labels.push(label);
        buf.advance(len as usize);
    }

    Ok(labels.join("."))
}

/// Skip domain name without parsing
fn skip_domain_name(buf: &mut &[u8]) -> Result<()> {
    loop {
        if buf.is_empty() {
            return Err(VerySlipError::Parse("Unexpected end of domain name".to_string()));
        }

        let len = buf[0];

        // Check for compression pointer
        if len & 0xC0 == 0xC0 {
            buf.advance(2);
            return Ok(());
        }

        buf.advance(1);

        if len == 0 {
            return Ok(());
        }

        if buf.remaining() < len as usize {
            return Err(VerySlipError::Parse("Truncated domain label".to_string()));
        }

        buf.advance(len as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_creation() {
        let payload = b"hello world".to_vec();
        let query = DnsQuery::new(12345, "example.com", payload.clone());
        
        assert_eq!(query.id, 12345);
        assert!(query.qname.ends_with(".example.com"));
        assert_eq!(query.qtype, RecordType::TXT);
        assert_eq!(query.payload, payload);
    }

    #[test]
    fn test_query_encode() {
        let query = DnsQuery::new(1, "test.com", b"data".to_vec());
        let encoded = query.encode().unwrap();
        
        assert!(encoded.len() >= 12); // At least header
        assert_eq!(&encoded[0..2], &[0, 1]); // ID = 1
    }

    #[test]
    fn test_max_payload_size() {
        let size = DnsQuery::max_payload_size("example.com", 1400);
        assert!(size > 0);
        assert!(size < 1400);
    }

    #[test]
    fn test_response_parse_empty() {
        let data = vec![
            0, 1, // ID
            0x81, 0x80, // Flags: QR=1, RD=1, RA=1
            0, 0, // QDCOUNT
            0, 0, // ANCOUNT
            0, 0, // NSCOUNT
            0, 0, // ARCOUNT
        ];
        
        let response = DnsResponse::parse(&data).unwrap();
        assert_eq!(response.id, 1);
        assert_eq!(response.answers.len(), 0);
    }
}
