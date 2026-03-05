# Very Slip Client Troubleshooting Guide

Common issues and solutions for Very Slip Client.

## Connection Issues

### Cannot Connect to Tunnel Server

**Symptoms**:
- Browser shows "Proxy server is refusing connections"
- Very Slip Client logs show "Connection failed" or "Timeout"

**Solutions**:

1. **Verify tunnel domains are correct**:
   ```bash
   # Test DNS resolution
   nslookup s1.example.com 8.8.8.8
   ```

2. **Check DNS resolvers are accessible**:
   ```bash
   # Test resolver connectivity
   dig @8.8.8.8 google.com
   ```

3. **Try different resolvers**:
   ```toml
   resolvers = ["1.1.1.1:53", "9.9.9.9:53", "208.67.222.222:53"]
   ```

4. **Check firewall allows UDP port 53**:
   ```bash
   # Linux
   sudo iptables -L | grep 53
   
   # Windows
   netsh advfirewall firewall show rule name=all | findstr 53
   ```

5. **Verify server is running**:
   - Contact your tunnel provider
   - Check server status page

### Proxy Connection Refused

**Symptoms**:
- Browser cannot connect to `127.0.0.1:8080`

**Solutions**:

1. **Check veryslip-client is running**:
   ```bash
   # Linux/macOS
   ps aux | grep veryslip-client
   
   # Windows
   tasklist | findstr veryslip-client
   ```

2. **Verify port is not in use**:
   ```bash
   # Linux/macOS
   lsof -i :8080
   
   # Windows
   netstat -ano | findstr :8080
   ```

3. **Try different port**:
   ```toml
   [proxy]
   port = 8081
   ```

4. **Check firewall allows local connections**:
   - Ensure localhost/127.0.0.1 is not blocked

## Performance Issues

### Slow Page Loads

**Symptoms**:
- Pages take >10 seconds to load
- High latency

**Solutions**:

1. **Increase query concurrency**:
   ```toml
   [query]
   concurrency = 16  # or 32 for high bandwidth
   ```

2. **Enable compression** (if disabled):
   ```toml
   [compression]
   enabled = true
   level = 5
   ```

3. **Enable caching** (if disabled):
   ```toml
   [cache]
   enabled = true
   max_memory_size = 524288000
   ```

4. **Add more tunnel domains**:
   ```toml
   domains = ["s1.example.com", "s2.example.com", "s3.example.com"]
   ```

5. **Check network latency**:
   ```bash
   ping 8.8.8.8
   ```

6. **Monitor metrics**:
   - Visit `http://localhost:9091/metrics`
   - Check `veryslip_rtt_seconds` for latency
   - Check `veryslip_cache_requests_total` for cache hit rate

### High CPU Usage

**Symptoms**:
- CPU usage >80%
- System becomes slow

**Solutions**:

1. **Reduce compression level**:
   ```toml
   [compression]
   level = 3  # Lower = faster, less compression
   ```

2. **Reduce query concurrency**:
   ```toml
   [query]
   concurrency = 4
   ```

3. **Disable prefetch**:
   ```toml
   [prefetch]
   enabled = false
   ```

4. **Disable ad blocking** (if not needed):
   ```toml
   [filter]
   enabled = false
   ```

5. **Check for CPU-intensive processes**:
   ```bash
   # Linux
   top
   
   # Windows
   taskmgr
   ```

### High Memory Usage

**Symptoms**:
- Memory usage >2GB
- System swapping

**Solutions**:

1. **Reduce cache size**:
   ```toml
   [cache]
   max_memory_size = 104857600  # 100MB
   ```

2. **Reduce buffer pool**:
   ```toml
   [buffer]
   max_size = 1000
   ```

3. **Reduce connection pool**:
   ```toml
   [connection]
   max_connections = 5
   ```

4. **Monitor memory usage**:
   ```bash
   # Linux
   ps aux | grep veryslip-client
   
   # Windows
   tasklist /FI "IMAGENAME eq veryslip-client.exe" /FO LIST
   ```

## Cache Issues

### Cache Not Working

**Symptoms**:
- Cache hit rate is 0%
- Same resources downloaded repeatedly

**Solutions**:

1. **Verify cache is enabled**:
   ```toml
   [cache]
   enabled = true
   ```

2. **Check disk space**:
   ```bash
   # Linux/macOS
   df -h ~/.config/veryslip-client/cache
   
   # Windows
   dir C:\Users\USERNAME\AppData\Roaming\veryslip-client\cache
   ```

3. **Verify cache path is writable**:
   ```bash
   # Linux/macOS
   ls -la ~/.config/veryslip-client/cache
   
   # Windows
   icacls C:\Users\USERNAME\AppData\Roaming\veryslip-client\cache
   ```

4. **Check cache size limit**:
   ```toml
   [cache]
   max_memory_size = 524288000  # Increase if needed
   ```

5. **Clear cache and restart**:
   ```bash
   # Linux/macOS
   rm -rf ~/.config/veryslip-client/cache/*
   
   # Windows
   del /Q C:\Users\USERNAME\AppData\Roaming\veryslip-client\cache\*
   ```

### Cache Growing Too Large

**Symptoms**:
- Disk space running out
- Cache directory >10GB

**Solutions**:

1. **Reduce cache size**:
   ```toml
   [cache]
   max_memory_size = 104857600  # 100MB
   ```

2. **Reduce cache TTL**:
   ```toml
   [cache]
   default_ttl_html_secs = 1800  # 30 minutes
   default_ttl_css_js_secs = 43200  # 12 hours
   default_ttl_images_secs = 86400  # 1 day
   ```

3. **Clear old cache entries**:
   - Restart veryslip-client (automatic cleanup on startup)

## Compression Issues

### Compression Not Working

**Symptoms**:
- No bandwidth savings
- `veryslip_compression_ratio` metric is 1.0

**Solutions**:

1. **Verify compression is enabled**:
   ```toml
   [compression]
   enabled = true
   ```

2. **Check server supports compression**:
   - Contact tunnel provider
   - Verify server has compression enabled

3. **Increase compression level**:
   ```toml
   [compression]
   level = 7
   ```

4. **Enable adaptive compression**:
   ```toml
   [compression]
   adaptive = true
   ```

### Compression Too Slow

**Symptoms**:
- High CPU usage
- Slow response times

**Solutions**:

1. **Reduce compression level**:
   ```toml
   [compression]
   level = 3
   ```

2. **Disable adaptive compression**:
   ```toml
   [compression]
   adaptive = false
   ```

## DNS Issues

### DNS Queries Failing

**Symptoms**:
- High failure rate in metrics
- Frequent timeouts

**Solutions**:

1. **Try different DNS resolvers**:
   ```toml
   resolvers = ["1.1.1.1:53", "8.8.8.8:53", "9.9.9.9:53"]
   ```

2. **Increase query timeout**:
   - Currently hardcoded to 5s
   - Reduce concurrency to avoid overwhelming resolver

3. **Check resolver rate limits**:
   - Some public resolvers have rate limits
   - Use multiple resolvers

4. **Verify UDP port 53 is not blocked**:
   ```bash
   # Test UDP connectivity
   nc -u 8.8.8.8 53
   ```

### MTU Discovery Failing

**Symptoms**:
- Queries failing with large payloads
- MTU stuck at minimum (900)

**Solutions**:

1. **Manually set MTU**:
   ```toml
   [mtu]
   min_size = 1200
   max_size = 1200  # Force specific MTU
   ```

2. **Increase probe count**:
   ```toml
   [mtu]
   probe_count = 10
   ```

3. **Check network MTU**:
   ```bash
   # Linux
   ip link show
   
   # Windows
   netsh interface ipv4 show subinterfaces
   ```

## Load Balancing Issues

### All Domains Marked Unavailable

**Symptoms**:
- "No available domains" error
- All domains showing as failed

**Solutions**:

1. **Check domain connectivity**:
   ```bash
   nslookup s1.example.com 8.8.8.8
   nslookup s2.example.com 8.8.8.8
   ```

2. **Reduce failure timeout**:
   ```toml
   [load_balancer]
   failure_timeout_secs = 30
   ```

3. **Lower success threshold**:
   ```toml
   [load_balancer]
   success_threshold = 0.3
   ```

4. **Verify server is running**:
   - Contact tunnel provider

### Uneven Load Distribution

**Symptoms**:
- One domain getting all traffic
- Other domains unused

**Solutions**:

1. **Check domain health**:
   - Monitor metrics for per-domain statistics
   - Verify all domains are responding

2. **Adjust weight reduction**:
   ```toml
   [load_balancer]
   weight_reduction = 0.5  # More aggressive reduction
   ```

3. **Increase window size**:
   ```toml
   [load_balancer]
   window_size_secs = 600  # 10 minutes
   ```

## Logging Issues

### No Logs Appearing

**Symptoms**:
- No output in terminal
- Log file empty

**Solutions**:

1. **Check log level**:
   ```toml
   [logging]
   level = "info"  # or "debug" for more detail
   ```

2. **Verify log output**:
   ```toml
   [logging]
   output = "stdout"  # or "both" for file + stdout
   ```

3. **Check file permissions**:
   ```bash
   # Linux/macOS
   ls -la ~/.config/veryslip-client/logs/
   
   # Windows
   icacls C:\Users\USERNAME\AppData\Roaming\veryslip-client\logs\
   ```

### Log File Too Large

**Symptoms**:
- Log file >1GB
- Disk space issues

**Solutions**:

1. **Reduce log level**:
   ```toml
   [logging]
   level = "warn"  # Only warnings and errors
   ```

2. **Reduce max file size**:
   ```toml
   [logging]
   file_max_size = 10485760  # 10MB
   ```

3. **Reduce file count**:
   ```toml
   [logging]
   file_max_count = 2
   ```

## Metrics Issues

### Metrics Endpoint Not Accessible

**Symptoms**:
- Cannot access `http://localhost:9091/metrics`

**Solutions**:

1. **Verify metrics are enabled**:
   ```toml
   [metrics]
   enabled = true
   ```

2. **Check port is not in use**:
   ```bash
   # Linux/macOS
   lsof -i :9091
   
   # Windows
   netstat -ano | findstr :9091
   ```

3. **Try different port**:
   ```toml
   [metrics]
   http_port = 9092
   ```

4. **Check firewall**:
   - Ensure localhost connections allowed

## Getting Help

If you're still experiencing issues:

1. **Enable debug logging**:
   ```toml
   [logging]
   level = "debug"
   output = "both"
   file_path = "/path/to/veryslip-client-debug.log"
   ```

2. **Collect diagnostics**:
   ```bash
   # System info
   uname -a  # Linux/macOS
   systeminfo  # Windows
   
   # Network info
   ifconfig  # Linux/macOS
   ipconfig  # Windows
   
   # Very Slip Client version
   veryslip-client --version
   ```

3. **Check metrics**:
   - Visit `http://localhost:9091/metrics`
   - Save output for analysis

4. **Report issue**:
   - Include debug logs
   - Include configuration (remove sensitive data)
   - Include metrics output
   - Describe expected vs actual behavior

## Common Error Messages

### "Configuration validation failed"
- Check config file syntax (valid TOML)
- Verify all required fields present
- Run: `veryslip-client --validate-config`

### "Failed to bind to port"
- Port already in use
- Try different port
- Check for other proxy software

### "No domains configured"
- Add at least one domain to config
- Verify `domains = [...]` is present

### "Failed to create QUIC client"
- TLS/certificate issue
- Check system time is correct
- Update system root certificates

### "Buffer pool exhausted"
- Increase buffer pool size
- Reduce query concurrency
- Check for memory leaks

### "Queue full"
- Reduce incoming request rate
- Increase queue size
- Check for bottlenecks
