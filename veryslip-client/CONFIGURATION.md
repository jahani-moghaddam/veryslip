# Very Slip Client Configuration Guide

Complete reference for all configuration options.

## Configuration File Format

Very Slip Client uses TOML format for configuration. Generate a default config with:

```bash
veryslip-client --generate-config > config.toml
```

## Core Settings

### Domains (Required)

```toml
domains = ["s1.example.com", "s2.example.com"]
```

- **Type**: Array of strings
- **Required**: Yes
- **Min**: 1 domain
- **Max**: 50 domains
- **Description**: Tunnel server domains for DNS queries. Multiple domains enable load balancing.

### Resolvers (Required)

```toml
resolvers = ["8.8.8.8:53", "1.1.1.1:53"]
```

- **Type**: Array of strings (IP:PORT format)
- **Required**: Yes
- **Description**: DNS resolvers to use for tunnel queries.
- **Common options**:
  - Google: `8.8.8.8:53`, `8.8.4.4:53`
  - Cloudflare: `1.1.1.1:53`, `1.0.0.1:53`
  - Quad9: `9.9.9.9:53`
  - OpenDNS: `208.67.222.222:53`

## Proxy Settings

```toml
[proxy]
port = 8080
auth_enabled = false
username = ""
password = ""
```

### port
- **Type**: Integer
- **Default**: 8080
- **Range**: 1-65535
- **Description**: Local HTTP proxy port

### auth_enabled
- **Type**: Boolean
- **Default**: false
- **Description**: Enable proxy authentication

### username / password
- **Type**: String
- **Default**: ""
- **Description**: Credentials for proxy authentication (if enabled)

## Compression Settings

```toml
[compression]
enabled = true
level = 5
adaptive = true
dictionary_path = ""
```

### enabled
- **Type**: Boolean
- **Default**: true
- **Description**: Enable Zstandard compression

### level
- **Type**: Integer
- **Default**: 5
- **Range**: 1-9
- **Description**: Compression level (higher = better compression, slower)
- **Recommendations**:
  - Level 3: Fast, good for high-bandwidth
  - Level 5: Balanced (recommended)
  - Level 7-9: Maximum compression, higher CPU

### adaptive
- **Type**: Boolean
- **Default**: true
- **Description**: Automatically adjust compression level based on content type

### dictionary_path
- **Type**: String (path)
- **Default**: "" (no dictionary)
- **Description**: Path to pre-trained compression dictionary for better compression

## Cache Settings

```toml
[cache]
enabled = true
max_memory_size = 524288000
disk_path = ""
default_ttl_html_secs = 3600
default_ttl_css_js_secs = 86400
default_ttl_images_secs = 604800
```

### enabled
- **Type**: Boolean
- **Default**: true
- **Description**: Enable HTTP caching

### max_memory_size
- **Type**: Integer (bytes)
- **Default**: 524288000 (500MB)
- **Description**: Maximum memory cache size

### disk_path
- **Type**: String (path)
- **Default**: "" (auto: ~/.config/veryslip-client/cache)
- **Description**: Path for persistent disk cache

### default_ttl_*_secs
- **Type**: Integer (seconds)
- **Defaults**:
  - HTML: 3600 (1 hour)
  - CSS/JS: 86400 (24 hours)
  - Images: 604800 (7 days)
- **Description**: Default cache TTL when server doesn't specify

## Query Engine Settings

```toml
[query]
concurrency = 8
max_in_flight = 1000
batch_timeout_ms = 5
batch_threshold = 0.8
```

### concurrency
- **Type**: Integer
- **Default**: 8
- **Range**: 1-32
- **Description**: Number of parallel DNS queries
- **Recommendations**:
  - Low bandwidth: 4-8
  - High bandwidth: 16-32

### max_in_flight
- **Type**: Integer
- **Default**: 1000
- **Description**: Maximum pending queries

### batch_timeout_ms
- **Type**: Integer (milliseconds)
- **Default**: 5
- **Description**: Wait time to accumulate packets for batching

### batch_threshold
- **Type**: Float
- **Default**: 0.8
- **Range**: 0.0-1.0
- **Description**: Send batch when it reaches this fraction of MTU

## DNS-over-HTTPS (DoH) Settings

```toml
[doh]
enabled = false
endpoints = ["https://cloudflare-dns.com/dns-query"]
timeout_secs = 5
use_get_method = false
```

### enabled
- **Type**: Boolean
- **Default**: false
- **Description**: Enable DNS-over-HTTPS for enhanced privacy. When disabled, uses traditional UDP DNS.

### endpoints
- **Type**: Array of strings (HTTPS URLs)
- **Default**: `["https://cloudflare-dns.com/dns-query"]`
- **Description**: DoH server endpoints. Multiple endpoints enable round-robin load balancing.
- **Well-known providers**:
  - Cloudflare: `https://cloudflare-dns.com/dns-query`
  - Google: `https://dns.google/dns-query`
  - Quad9: `https://dns.quad9.net/dns-query`
  - OpenDNS: `https://doh.opendns.com/dns-query`

### timeout_secs
- **Type**: Integer (seconds)
- **Default**: 5
- **Range**: 1-30
- **Description**: DoH query timeout

### use_get_method
- **Type**: Boolean
- **Default**: false
- **Description**: Use GET method instead of POST (RFC 8484). POST is recommended for better privacy.

**Note**: When DoH is enabled, it takes precedence over UDP DNS. If DoH queries fail, the system automatically falls back to UDP DNS using the configured resolvers.

## MTU Settings

```toml
[mtu]
min_size = 900
max_size = 1400
probe_count = 5
reprobe_threshold = 0.1
```

### min_size / max_size
- **Type**: Integer (bytes)
- **Defaults**: 900 / 1400
- **Description**: MTU discovery range

### probe_count
- **Type**: Integer
- **Default**: 5
- **Description**: Number of test queries per MTU size

### reprobe_threshold
- **Type**: Float
- **Default**: 0.1
- **Description**: Trigger reprobe if failure rate exceeds this

## Load Balancer Settings

```toml
[load_balancer]
failure_timeout_secs = 60
window_size_secs = 300
success_threshold = 0.5
weight_reduction = 0.75
```

### failure_timeout_secs
- **Type**: Integer (seconds)
- **Default**: 60
- **Description**: Mark domain unavailable for this duration after failure

### window_size_secs
- **Type**: Integer (seconds)
- **Default**: 300 (5 minutes)
- **Description**: Sliding window for success rate calculation

### success_threshold
- **Type**: Float
- **Default**: 0.5
- **Range**: 0.0-1.0
- **Description**: Minimum success rate to keep full weight

### weight_reduction
- **Type**: Float
- **Default**: 0.75
- **Range**: 0.0-1.0
- **Description**: Reduce allocation by this factor for degraded domains

## Priority Settings

```toml
[priority]
bandwidth_weights = [0.4, 0.3, 0.2, 0.1]
starvation_timeout_secs = 30
```

### bandwidth_weights
- **Type**: Array of 4 floats
- **Default**: [0.4, 0.3, 0.2, 0.1]
- **Description**: Bandwidth allocation for [Critical, High, Medium, Low]
- **Must sum to 1.0**

### starvation_timeout_secs
- **Type**: Integer (seconds)
- **Default**: 30
- **Description**: Elevate Low priority requests after this timeout

## Connection Settings

```toml
[connection]
max_connections = 10
idle_timeout_secs = 60
```

### max_connections
- **Type**: Integer
- **Default**: 10
- **Range**: 1-100
- **Description**: Maximum concurrent QUIC connections

### idle_timeout_secs
- **Type**: Integer (seconds)
- **Default**: 60
- **Description**: Close idle connections after this timeout

## Buffer Settings

```toml
[buffer]
initial_size = 100
max_size = 10000
buffer_capacity = 2048
```

### initial_size
- **Type**: Integer
- **Default**: 100
- **Description**: Initial buffer pool size

### max_size
- **Type**: Integer
- **Default**: 10000
- **Description**: Maximum buffer pool size

### buffer_capacity
- **Type**: Integer (bytes)
- **Default**: 2048
- **Description**: Size of each buffer

## Prefetch Settings

```toml
[prefetch]
enabled = true
max_queue_size = 50
resource_types = ["stylesheet", "script", "image"]
```

### enabled
- **Type**: Boolean
- **Default**: true
- **Description**: Enable HTML resource prefetching

### max_queue_size
- **Type**: Integer
- **Default**: 50
- **Description**: Maximum prefetch queue size

### resource_types
- **Type**: Array of strings
- **Default**: ["stylesheet", "script", "image"]
- **Options**: "stylesheet", "script", "image", "font"
- **Description**: Resource types to prefetch

## Filter Settings

```toml
[filter]
enabled = true
blocklist_path = ""
```

### enabled
- **Type**: Boolean
- **Default**: true
- **Description**: Enable ad/tracker blocking

### blocklist_path
- **Type**: String (path)
- **Default**: "" (use embedded EasyList)
- **Description**: Path to custom blocklist file (one domain per line)

## Metrics Settings

```toml
[metrics]
enabled = true
http_port = 9091
log_interval_secs = 60
```

### enabled
- **Type**: Boolean
- **Default**: true
- **Description**: Enable metrics collection

### http_port
- **Type**: Integer
- **Default**: 9091
- **Description**: Prometheus metrics HTTP endpoint port

### log_interval_secs
- **Type**: Integer (seconds)
- **Default**: 60
- **Description**: Log metrics summary interval

## Logging Settings

```toml
[logging]
level = "info"
format = "text"
output = "stdout"
file_path = ""
file_max_size = 104857600
file_max_count = 5
```

### level
- **Type**: String
- **Default**: "info"
- **Options**: "error", "warn", "info", "debug", "trace"
- **Description**: Log level

### format
- **Type**: String
- **Default**: "text"
- **Options**: "text", "json"
- **Description**: Log format

### output
- **Type**: String
- **Default**: "stdout"
- **Options**: "stdout", "file", "both"
- **Description**: Log output destination

### file_path
- **Type**: String (path)
- **Default**: "" (auto: ~/.config/veryslip-client/logs/veryslip-client.log)
- **Description**: Log file path (if output includes "file")

### file_max_size
- **Type**: Integer (bytes)
- **Default**: 104857600 (100MB)
- **Description**: Rotate log file at this size

### file_max_count
- **Type**: Integer
- **Default**: 5
- **Description**: Keep this many rotated log files

## Example Configurations

### Minimal (Single Domain)

```toml
domains = ["tunnel.example.com"]
resolvers = ["8.8.8.8:53"]
```

### Balanced (Recommended)

```toml
domains = ["s1.example.com", "s2.example.com", "s3.example.com"]
resolvers = ["8.8.8.8:53", "1.1.1.1:53"]

[proxy]
port = 8080

[compression]
enabled = true
level = 5

[cache]
enabled = true
max_memory_size = 524288000

[filter]
enabled = true

[prefetch]
enabled = true

[metrics]
enabled = true
```

### Maximum Performance

```toml
domains = ["s1.example.com", "s2.example.com", "s3.example.com", "s4.example.com", "s5.example.com"]
resolvers = ["8.8.8.8:53", "1.1.1.1:53"]

[query]
concurrency = 32

[compression]
enabled = true
level = 3
adaptive = true

[cache]
enabled = true
max_memory_size = 1073741824  # 1GB

[connection]
max_connections = 20

[prefetch]
enabled = true
max_queue_size = 100
```

### Low Resource

```toml
domains = ["tunnel.example.com"]
resolvers = ["8.8.8.8:53"]

[query]
concurrency = 4

[compression]
enabled = true
level = 3

[cache]
enabled = true
max_memory_size = 104857600  # 100MB

[prefetch]
enabled = false

[filter]
enabled = false
```
