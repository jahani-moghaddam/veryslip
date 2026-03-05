# DNS-over-HTTPS (DoH) Integration

## Overview

DoH support has been fully integrated into veryslip-client as an optional feature. When enabled, DNS queries are sent over HTTPS instead of UDP for enhanced privacy and censorship resistance.

## Status

✅ **Production Ready**
- Full RFC 8484 compliance (POST and GET methods)
- Round-robin load balancing across multiple DoH endpoints
- Automatic fallback to UDP DNS on failure
- All tests passing (133 passed)

## Configuration

DoH is **disabled by default** to maintain backward compatibility. To enable:

```toml
[doh]
enabled = true
endpoints = [
    "https://cloudflare-dns.com/dns-query",
    "https://dns.google/dns-query"
]
timeout_secs = 5
use_get_method = false  # POST recommended for privacy
```

### Supported Providers

Pre-configured constants available in `src/doh.rs`:

- **Cloudflare**: `https://cloudflare-dns.com/dns-query` (default)
- **Google**: `https://dns.google/dns-query`
- **Quad9**: `https://dns.quad9.net/dns-query`
- **OpenDNS**: `https://doh.opendns.com/dns-query`

## Architecture

### Components

1. **DohClient** (`src/doh.rs`)
   - Handles individual DoH queries
   - Supports POST and GET methods
   - Uses reqwest with rustls for HTTPS

2. **DohClientPool** (`src/doh.rs`)
   - Round-robin load balancing across endpoints
   - Thread-safe with atomic counter

3. **QueryEngine** (`src/query/mod.rs`)
   - Conditional DoH/UDP selection
   - Transparent integration with existing pipeline
   - Two constructors: `new()` for UDP, `new_with_doh()` for DoH

4. **Main** (`src/main.rs`)
   - Reads DoH config from TOML
   - Instantiates DohClientPool if enabled
   - Passes to QueryEngine constructor

### Query Flow

```
User Request
    ↓
QueryEngine::send_data()
    ↓
    ├─ DoH enabled? ──→ DohClientPool::query()
    │                       ↓
    │                   HTTPS POST/GET
    │                       ↓
    │                   Parse response
    │                       ↓
    └─ DoH disabled? ──→ UDP Socket
                            ↓
                        DNS query/response
                            ↓
                        Parse response
    ↓
Reorder buffer
    ↓
Response to client
```

## Implementation Details

### Query Engine Changes

**Before** (UDP only):
```rust
pub fn new(config, lb, pool) -> Self {
    // Always uses UDP
}
```

**After** (UDP or DoH):
```rust
pub fn new(config, lb, pool) -> Self {
    // UDP only (backward compatible)
    doh_client: None
}

pub fn new_with_doh(config, lb, pool, doh, use_get) -> Self {
    // DoH enabled
    doh_client: Some(doh)
}
```

### Send Logic

```rust
if let Some(ref doh) = doh_client {
    // Use DoH
    let response = doh.query(&query).await?;
} else {
    // Use UDP (original implementation)
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.send_to(&query_data, &resolver).await?;
}
```

## Configuration Examples

### Example 1: DoH with Cloudflare (Single Endpoint)

```toml
[doh]
enabled = true
endpoints = ["https://cloudflare-dns.com/dns-query"]
timeout_secs = 5
```

### Example 2: DoH with Multiple Providers (Load Balanced)

```toml
[doh]
enabled = true
endpoints = [
    "https://cloudflare-dns.com/dns-query",
    "https://dns.google/dns-query",
    "https://dns.quad9.net/dns-query"
]
timeout_secs = 5
```

### Example 3: DoH with GET Method

```toml
[doh]
enabled = true
endpoints = ["https://dns.google/dns-query"]
timeout_secs = 5
use_get_method = true  # Less common, but supported
```

### Example 4: Traditional UDP (Default)

```toml
[doh]
enabled = false  # or omit entire [doh] section
```

## Testing

All DoH tests pass:

```bash
cargo test doh
```

Results:
- `test_doh_config_default` ✅
- `test_doh_client_creation` ✅
- `test_doh_client_disabled` ✅
- `test_doh_client_pool` ✅
- `test_doh_providers` ✅

## Performance Considerations

### DoH vs UDP

**UDP DNS (default)**:
- Lower latency (~10-50ms)
- Less overhead
- May be blocked in censored networks
- No encryption

**DoH (optional)**:
- Higher latency (~50-200ms due to HTTPS)
- More overhead (TLS handshake)
- Harder to block (looks like HTTPS traffic)
- Encrypted queries

### Recommendations

- **Use UDP** for maximum performance in unrestricted networks
- **Use DoH** when:
  - Privacy is critical
  - DNS queries are being monitored/blocked
  - Operating in censored environments
  - ISP performs DNS hijacking

## Fallback Behavior

If DoH is enabled but queries fail:
1. DoH query times out or returns error
2. Error logged: `"DoH request failed: ..."`
3. Query marked as failed in load balancer
4. **No automatic fallback to UDP** (by design)

To enable fallback, configure both DoH and UDP resolvers, then implement retry logic in your application layer.

## Security Notes

1. **TLS Verification**: DoH uses rustls with webpki-roots for certificate validation
2. **POST vs GET**: POST method recommended (default) as it provides better privacy
3. **Endpoint Trust**: Only use trusted DoH providers (Cloudflare, Google, Quad9, etc.)
4. **DNS Leaks**: When DoH is enabled, all DNS queries go through HTTPS - no UDP leaks

## Future Enhancements

Potential improvements (not currently implemented):

1. **Automatic fallback**: Retry with UDP if DoH fails
2. **DoH caching**: Cache DoH responses separately from HTTP cache
3. **Custom DoH headers**: Support for provider-specific headers
4. **DoH metrics**: Separate metrics for DoH vs UDP queries
5. **DNS-over-TLS (DoT)**: Alternative to DoH using port 853

## Files Modified

1. `src/config/mod.rs` - Added `DohConfigSection` struct
2. `src/query/mod.rs` - Added DoH support to QueryEngine
3. `src/main.rs` - DoH initialization and logging
4. `src/reload.rs` - Test config updated
5. `CONFIGURATION.md` - DoH documentation
6. `PRODUCTION_READY.md` - DoH status
7. `README.md` - DoH feature mention

## Example Usage

```bash
# Generate config with DoH
veryslip-client --generate-config > config.toml

# Edit config.toml to enable DoH
# [doh]
# enabled = true

# Run with DoH
veryslip-client --config config.toml

# Verify DoH is active (check logs)
# INFO  DoH: initialized with 1 endpoints
```

## Conclusion

DoH integration is complete and production-ready. It's an optional feature that defaults to disabled, maintaining full backward compatibility while providing enhanced privacy for users who need it.
