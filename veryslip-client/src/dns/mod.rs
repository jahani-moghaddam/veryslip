pub mod base32;
pub mod packet;

pub use base32::{encode_base32, decode_base32, insert_dots, remove_dots};
pub use packet::{DnsQuery, DnsResponse, DnsAnswer};

/// DNS record types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    A = 1,
    NS = 2,
    CNAME = 5,
    SOA = 6,
    PTR = 12,
    MX = 15,
    TXT = 16,
    AAAA = 28,
    OPT = 41,
}

impl RecordType {
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(RecordType::A),
            2 => Some(RecordType::NS),
            5 => Some(RecordType::CNAME),
            6 => Some(RecordType::SOA),
            12 => Some(RecordType::PTR),
            15 => Some(RecordType::MX),
            16 => Some(RecordType::TXT),
            28 => Some(RecordType::AAAA),
            41 => Some(RecordType::OPT),
            _ => None,
        }
    }
}

/// DNS class types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    IN = 1,
    CS = 2,
    CH = 3,
    HS = 4,
}

impl Class {
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Class::IN),
            2 => Some(Class::CS),
            3 => Some(Class::CH),
            4 => Some(Class::HS),
            _ => None,
        }
    }
}

/// DNS response codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseCode {
    NoError = 0,
    FormatError = 1,
    ServerFailure = 2,
    NameError = 3,
    NotImplemented = 4,
    Refused = 5,
}

impl ResponseCode {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => ResponseCode::NoError,
            1 => ResponseCode::FormatError,
            2 => ResponseCode::ServerFailure,
            3 => ResponseCode::NameError,
            4 => ResponseCode::NotImplemented,
            5 => ResponseCode::Refused,
            _ => ResponseCode::ServerFailure,
        }
    }
}

/// Maximum DNS query size (traditional UDP limit)
pub const MAX_DNS_QUERY_SIZE: usize = 512;

/// EDNS0 UDP payload size
pub const EDNS0_UDP_PAYLOAD: u16 = 1232;

/// Insert dots every N characters for DNS label length compliance
pub const DOT_INTERVAL: usize = 57;
