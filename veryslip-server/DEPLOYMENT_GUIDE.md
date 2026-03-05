# VerySlip Server Deployment Guide

## Quick Start

Deploy veryslip-server on a Linux VPS with one command:

```bash
sudo bash veryslip-server-deploy.sh
```

## Prerequisites

- Linux VPS (Ubuntu 20.04+, Debian 11+, CentOS 8+, or RHEL 8+)
- Root or sudo access
- At least 1GB RAM
- 10GB disk space
- Public IP address
- Domain name pointing to your server

## Manual Installation

If you prefer manual installation or need to customize the deployment:

### 1. Install Dependencies

**Ubuntu/Debian:**
```bash
sudo apt-get update
sudo apt-get install -y curl build-essential cmake pkg-config libssl-dev git
```

**CentOS/RHEL/Fedora:**
```bash
sudo yum install -y curl gcc gcc-c++ make cmake pkg-config openssl-devel git
```

### 2. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

### 3. Clone and Build

```bash
git clone https://github.com/yourusername/veryslip-server.git
cd veryslip-server
cargo build --release
```

The binary will be at `target/release/slipstream-server`.

### 4. Generate Certificates

```bash
mkdir -p /etc/veryslip-server/certs
cd /etc/veryslip-server/certs

# Generate ECDSA P-256 key and self-signed certificate
openssl ecparam -genkey -name prime256v1 -out key.pem
openssl req -new -x509 -key key.pem -out cert.pem -days 3650 \
    -subj "/C=US/ST=State/L=City/O=Organization/CN=yourdomain.com"

chmod 600 key.pem
chmod 644 cert.pem
```

**For production**, replace with proper certificates from Let's Encrypt or your CA.

### 5. Create Service User

```bash
sudo useradd --system --no-create-home --shell /bin/false veryslip
sudo chown -R veryslip:veryslip /etc/veryslip-server/certs
```

### 6. Create Systemd Service

Create `/etc/systemd/system/veryslip-server.service`:

```ini
[Unit]
Description=VerySlip DNS Tunnel Server
After=network.target

[Service]
Type=simple
User=veryslip
Group=veryslip
ExecStart=/opt/veryslip-server/veryslip-server-bin \
    --dns-listen-host :: \
    --dns-listen-port 8853 \
    --target-address 127.0.0.1:8080 \
    --domain yourdomain.com \
    --cert /etc/veryslip-server/certs/cert.pem \
    --key /etc/veryslip-server/certs/key.pem \
    --compression-level 5 \
    --metrics-port 9090 \
    --max-connections 256 \
    --idle-timeout-seconds 60
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true

# Resource limits
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

### 7. Configure Firewall

**UFW (Ubuntu/Debian):**
```bash
sudo ufw allow 8853/udp comment "VerySlip DNS"
sudo ufw allow 9090/tcp comment "VerySlip Metrics"
```

**firewalld (CentOS/RHEL):**
```bash
sudo firewall-cmd --permanent --add-port=8853/udp
sudo firewall-cmd --permanent --add-port=9090/tcp
sudo firewall-cmd --reload
```

### 8. System Tuning

Add to `/etc/sysctl.conf`:

```ini
# VerySlip Server Tuning
net.core.rmem_max = 26214400
net.core.rmem_default = 26214400
net.core.wmem_max = 26214400
net.core.wmem_default = 26214400
net.core.netdev_max_backlog = 5000
net.ipv4.udp_mem = 10240 87380 26214400
```

Apply changes:
```bash
sudo sysctl -p
```

### 9. Start Service

```bash
sudo systemctl daemon-reload
sudo systemctl enable veryslip-server
sudo systemctl start veryslip-server
```

## Configuration

### Environment Variables

You can customize the deployment by setting environment variables before running the script:

```bash
export DOMAIN="yourdomain.com"
export DNS_PORT="8853"
export TARGET_ADDRESS="127.0.0.1:8080"
export METRICS_PORT="9090"
export COMPRESSION_LEVEL="5"
export MAX_CONNECTIONS="256"
export IDLE_TIMEOUT="60"

sudo -E bash veryslip-server-deploy.sh
```

### CLI Arguments

All configuration options:

```bash
--dns-listen-host <HOST>          # Listen address (default: ::)
--dns-listen-port <PORT>          # DNS port (default: 53)
--target-address <HOST:PORT>      # Target to forward to
--domain <DOMAIN>                 # Tunnel domain (can specify multiple)
--cert <PATH>                     # TLS certificate path
--key <PATH>                      # TLS key path
--compression-level <1-9>         # Compression level (default: 5)
--disable-compression             # Disable compression
--metrics-port <PORT>             # Metrics HTTP port (default: 9090)
--disable-metrics                 # Disable metrics
--max-connections <N>             # Max concurrent connections (default: 256)
--idle-timeout-seconds <N>        # Idle timeout (default: 60)
--debug-streams                   # Enable stream debugging
--debug-commands                  # Enable command debugging
```

## Service Management

### Start/Stop/Restart

```bash
sudo systemctl start veryslip-server
sudo systemctl stop veryslip-server
sudo systemctl restart veryslip-server
```

### Check Status

```bash
sudo systemctl status veryslip-server
```

### View Logs

```bash
# Follow logs in real-time
sudo journalctl -u veryslip-server -f

# View last 100 lines
sudo journalctl -u veryslip-server -n 100

# View logs since boot
sudo journalctl -u veryslip-server -b
```

## Monitoring

### Health Check

```bash
curl http://localhost:9090/health
```

Expected output: `OK`

### Metrics

```bash
curl http://localhost:9090/metrics
```

Returns Prometheus-format metrics including:
- `veryslip_bytes_sent_total` - Total bytes sent
- `veryslip_bytes_received_total` - Total bytes received
- `veryslip_queries_total` - Total queries processed
- `veryslip_queries_batched_total` - Total batched queries
- `veryslip_active_connections` - Current active connections
- `veryslip_compression_ratio` - Compression ratio
- `veryslip_rtt_seconds` - RTT histogram
- `veryslip_domain_queries_total{domain="..."}` - Per-domain queries

### Prometheus Integration

Add to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'veryslip-server'
    static_configs:
      - targets: ['localhost:9090']
    scrape_interval: 15s
```

### Grafana Dashboard

Import the VerySlip Server dashboard (coming soon) or create custom panels using the metrics above.

## Troubleshooting

### Service Won't Start

1. Check logs:
   ```bash
   sudo journalctl -u veryslip-server -n 50
   ```

2. Verify binary exists and is executable:
   ```bash
   ls -la /opt/veryslip-server/veryslip-server-bin
   ```

3. Check certificate permissions:
   ```bash
   ls -la /etc/veryslip-server/certs/
   ```

4. Verify port is not in use:
   ```bash
   sudo netstat -tulpn | grep 8853
   ```

### Connection Issues

1. Check firewall:
   ```bash
   sudo ufw status  # Ubuntu/Debian
   sudo firewall-cmd --list-all  # CentOS/RHEL
   ```

2. Verify DNS port is listening:
   ```bash
   sudo netstat -ulpn | grep 8853
   ```

3. Test with dig:
   ```bash
   dig @your-server-ip -p 8853 test.yourdomain.com TXT
   ```

### Performance Issues

1. Check system resources:
   ```bash
   top
   free -h
   df -h
   ```

2. Monitor connection count:
   ```bash
   curl http://localhost:9090/metrics | grep active_connections
   ```

3. Check for errors in logs:
   ```bash
   sudo journalctl -u veryslip-server | grep -i error
   ```

### Certificate Issues

1. Verify certificate validity:
   ```bash
   openssl x509 -in /etc/veryslip-server/certs/cert.pem -text -noout
   ```

2. Check certificate expiration:
   ```bash
   openssl x509 -in /etc/veryslip-server/certs/cert.pem -enddate -noout
   ```

3. Regenerate if needed:
   ```bash
   cd /etc/veryslip-server/certs
   sudo rm cert.pem key.pem
   # Run certificate generation commands from step 4 above
   sudo systemctl restart veryslip-server
   ```

## Security Hardening

### 1. Use Proper Certificates

Replace self-signed certificates with Let's Encrypt:

```bash
sudo apt-get install certbot
sudo certbot certonly --standalone -d yourdomain.com
sudo cp /etc/letsencrypt/live/yourdomain.com/fullchain.pem /etc/veryslip-server/certs/cert.pem
sudo cp /etc/letsencrypt/live/yourdomain.com/privkey.pem /etc/veryslip-server/certs/key.pem
sudo chown veryslip:veryslip /etc/veryslip-server/certs/*.pem
sudo systemctl restart veryslip-server
```

### 2. Restrict Metrics Access

Use firewall to restrict metrics port to localhost or specific IPs:

```bash
sudo ufw delete allow 9090/tcp
sudo ufw allow from 10.0.0.0/8 to any port 9090 proto tcp
```

### 3. Enable SELinux/AppArmor

Follow your distribution's guidelines for SELinux or AppArmor policies.

### 4. Regular Updates

```bash
cd /opt/veryslip-server/veryslip-server
git pull
cargo build --release
sudo cp target/release/slipstream-server /opt/veryslip-server/veryslip-server-bin
sudo systemctl restart veryslip-server
```

## Backup and Recovery

### Backup

```bash
# Backup certificates
sudo tar -czf veryslip-backup-$(date +%Y%m%d).tar.gz \
    /etc/veryslip-server/certs/ \
    /etc/systemd/system/veryslip-server.service
```

### Restore

```bash
sudo tar -xzf veryslip-backup-YYYYMMDD.tar.gz -C /
sudo systemctl daemon-reload
sudo systemctl restart veryslip-server
```

## Upgrade Procedure

1. Stop the service:
   ```bash
   sudo systemctl stop veryslip-server
   ```

2. Backup current installation:
   ```bash
   sudo cp /opt/veryslip-server/veryslip-server-bin /opt/veryslip-server/veryslip-server-bin.backup
   ```

3. Build new version:
   ```bash
   cd /opt/veryslip-server/veryslip-server
   git pull
   cargo build --release
   sudo cp target/release/slipstream-server /opt/veryslip-server/veryslip-server-bin
   ```

4. Start the service:
   ```bash
   sudo systemctl start veryslip-server
   ```

5. Verify:
   ```bash
   sudo systemctl status veryslip-server
   curl http://localhost:9090/health
   ```

## Rollback Procedure

If upgrade fails:

```bash
sudo systemctl stop veryslip-server
sudo cp /opt/veryslip-server/veryslip-server-bin.backup /opt/veryslip-server/veryslip-server-bin
sudo systemctl start veryslip-server
```

## Support

For issues and questions:
- GitHub Issues: https://github.com/yourusername/veryslip-server/issues
- Documentation: https://github.com/yourusername/veryslip-server/wiki
