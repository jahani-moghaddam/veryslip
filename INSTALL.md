# VerySlip Server - Installation Guide

## Quick Install (Recommended)

Install veryslip-server on your Linux VPS with one command:

```bash
sudo bash <(wget -qO- https://raw.githubusercontent.com/yourusername/veryslip/main/veryslip-server-deploy.sh)
```

Or download and run:

```bash
wget https://raw.githubusercontent.com/yourusername/veryslip/main/veryslip-server-deploy.sh
sudo bash veryslip-server-deploy.sh
```

## What You Need

### 1. A Linux VPS
- Ubuntu 20.04+ / Debian 11+ / CentOS 8+ / RHEL 8+
- At least 1GB RAM
- 10GB disk space
- Root or sudo access

### 2. A Domain Name
You need a domain name with DNS control. Examples:
- **Single domain**: `tunnel.example.com`
- **Multiple domains** (recommended): `t1.example.com, t2.example.com, t3.example.com`

Multiple domains provide 2-3x better performance through load balancing.

### 3. DNS Configuration (IMPORTANT!)

**Before running the installation script**, configure your DNS records:

#### For Single Domain

If your domain is `tunnel.example.com` and server IP is `1.2.3.4`:

```
Type: A     | Name: ns.tunnel      | Content: 1.2.3.4
Type: NS    | Name: tunnel         | Content: ns.tunnel.example.com
```

Or if using root domain:

```
Type: A     | Name: ns             | Content: 1.2.3.4
Type: NS    | Name: @              | Content: ns.example.com
```

#### For Multiple Domains (Recommended)

If you use `t1.example.com, t2.example.com, t3.example.com` with server IP `1.2.3.4`:

```
Type: A     | Name: ns.t1          | Content: 1.2.3.4
Type: NS    | Name: t1             | Content: ns.t1.example.com

Type: A     | Name: ns.t2          | Content: 1.2.3.4
Type: NS    | Name: t2             | Content: ns.t2.example.com

Type: A     | Name: ns.t3          | Content: 1.2.3.4
Type: NS    | Name: t3             | Content: ns.t3.example.com
```

#### DNS Provider Examples

**Cloudflare:**
1. Go to DNS settings
2. Add A record: `ns` → `1.2.3.4`
3. Add NS record: `@` or subdomain → `ns.yourdomain.com`

**Namecheap:**
1. Go to Advanced DNS
2. Add A record: `ns` → `1.2.3.4`
3. Add NS record: `@` or subdomain → `ns.yourdomain.com`

**Note:** DNS propagation takes 5-60 minutes. You can check with:
```bash
dig @8.8.8.8 yourdomain.com NS
```

## Installation Steps

### Step 1: Run the Installation Script

```bash
sudo bash <(wget -qO- https://raw.githubusercontent.com/yourusername/veryslip/main/veryslip-server-deploy.sh)
```

### Step 2: Answer the Questions

The script will ask you:

1. **Domain(s)**: Enter one or more domains (comma-separated)
   - Single: `tunnel.example.com`
   - Multiple: `t1.example.com, t2.example.com, t3.example.com`

2. **Tunnel mode**: Choose one:
   - `1` - SOCKS proxy (recommended)
   - `2` - SSH mode
   - `3` - Shadowsocks

### Step 3: DNS Configuration Check

The script will show you the DNS records you need to configure and ask for confirmation:

```
DNS Configuration Required
==========================

Your server IP: 1.2.3.4

For each domain, add these DNS records:

Domain 1: t1.example.com
  1. Add A record:
     ns.t1.example.com  →  1.2.3.4
  
  2. Add NS record:
     t1.example.com  →  ns.t1.example.com

Domain 2: t2.example.com
  1. Add A record:
     ns.t2.example.com  →  1.2.3.4
  
  2. Add NS record:
     t2.example.com  →  ns.t2.example.com

Have you configured DNS records? Continue installation? (y/N):
```

Type `y` and press Enter to continue.

### Step 4: Wait for Installation

The script will automatically:
- ✓ Install all dependencies (Rust, cmake, OpenSSL, etc.)
- ✓ Build the server (5-10 minutes)
- ✓ Generate TLS certificates
- ✓ Configure firewall
- ✓ Set up systemd service
- ✓ Install tunnel backend (Dante SOCKS if mode 1 selected)
- ✓ Start the server

## What Gets Installed

### Files and Directories

```
/opt/veryslip-server/              # Installation directory
  ├── veryslip-server-bin          # Server binary
  └── veryslip-server/             # Source code

/etc/veryslip-server/              # Configuration directory
  └── certs/                       # TLS certificates
      ├── cert.pem                 # Certificate
      └── key.pem                  # Private key

/etc/systemd/system/               # Systemd service
  └── veryslip-server.service      # Service file
```

### System User

A dedicated system user `veryslip` is created to run the service securely.

### Firewall Rules

The script automatically opens:
- Port `8853/udp` - DNS tunnel traffic
- Port `9090/tcp` - Metrics endpoint (localhost only recommended)

## How Client Connects to Server

The veryslip-client uses DNS to automatically find your server:

1. **You configure domains**: `domains = ["t1.example.com"]`
2. **You configure DNS resolvers**: `resolvers = ["8.8.8.8:53", "1.1.1.1:53"]`
3. **Client queries DNS**: Uses the resolvers to look up `t1.example.com NS`
4. **DNS returns NS record**: Points to `ns.t1.example.com`
5. **Client resolves NS**: Gets your server IP from the A record
6. **Client connects**: Establishes QUIC connection to your server on port 8853

This is why you need to configure NS records in your DNS settings - they tell the client where to find your server.

## After Installation

### 1. Verify Tunnel Backend

The script automatically sets up the tunnel backend based on your selection:

**SOCKS Proxy (Mode 1):**
```bash
# Dante SOCKS server is automatically installed and configured
# Check status
sudo systemctl status danted

# View logs
sudo journalctl -u danted -f

# Verify it's listening on port 1080
sudo netstat -tulpn | grep 1080
```

The Dante configuration is at `/etc/danted.conf` and is pre-configured to:
- Listen on 127.0.0.1:1080 (localhost only)
- Accept connections from veryslip-server
- Forward traffic through your primary network interface

**SSH Mode (Mode 2):**
```bash
# SSH server should already be running
sudo systemctl status sshd

# If not installed:
sudo apt install openssh-server
sudo systemctl start sshd
sudo systemctl enable sshd
```

**Shadowsocks (Mode 3):**
```bash
# Install Shadowsocks manually
sudo apt install shadowsocks-libev

# Configure /etc/shadowsocks-libev/config.json
sudo nano /etc/shadowsocks-libev/config.json

# Example config:
{
    "server": "127.0.0.1",
    "server_port": 8388,
    "password": "your-password",
    "method": "aes-256-gcm",
    "timeout": 300
}

# Start service
sudo systemctl start shadowsocks-libev
sudo systemctl enable shadowsocks-libev
```

### 2. Verify DNS Propagation

Check if your DNS records are working:

```bash
# For single domain
dig @8.8.8.8 tunnel.example.com NS

# For multiple domains
dig @8.8.8.8 t1.example.com NS
dig @8.8.8.8 t2.example.com NS
dig @8.8.8.8 t3.example.com NS
```

You should see your NS records pointing to the respective `ns.*.example.com` addresses.

### 3. Check Service Status

```bash
# Check if service is running
sudo systemctl status veryslip-server

# View logs
sudo journalctl -u veryslip-server -f

# Check metrics
curl http://localhost:9090/health
curl http://localhost:9090/metrics
```

### 4. Configure veryslip-client

On your client machine, configure veryslip-client to use your server:

**Single domain:**
```toml
# config.toml
domains = ["tunnel.example.com"]
resolvers = ["8.8.8.8:53", "1.1.1.1:53"]

[proxy]
port = 7000

[connection]
max_connections = 10
```

**Multiple domains:**
```toml
# config.toml
domains = [
    "t1.example.com",
    "t2.example.com",
    "t3.example.com",
]
resolvers = ["8.8.8.8:53", "1.1.1.1:53"]

[proxy]
port = 7000

[connection]
max_connections = 10
```

**Note**: The `resolvers` field specifies DNS resolvers (like Google DNS 8.8.8.8:53 or Cloudflare 1.1.1.1:53), NOT your server IP. The client uses these DNS resolvers to look up your domain's NS records, which point to your server automatically.

### 5. Run veryslip-client

On your client machine:

```bash
# Create config file
cat > config.toml << EOF
domains = ["t1.example.com", "t2.example.com", "t3.example.com"]
resolvers = ["8.8.8.8:53", "1.1.1.1:53"]

[proxy]
port = 8080
EOF

# Run the client
veryslip-client --config config.toml
```

Or use command-line arguments:

```bash
# Single domain
veryslip-client --domain t1.example.com --dns 8.8.8.8:53

# Multiple domains
veryslip-client --domain t1.example.com --domain t2.example.com --domain t3.example.com --dns 8.8.8.8:53 --dns 1.1.1.1:53
```

The client will:
- Start HTTP proxy on port 8080 (or specified port)
- Use DNS resolvers to find your server via NS records
- Establish QUIC connections to your server
- Tunnel all traffic through DNS

Configure your browser to use proxy `127.0.0.1:8080`.

## Service Management

### Start/Stop/Restart

```bash
sudo systemctl start veryslip-server
sudo systemctl stop veryslip-server
sudo systemctl restart veryslip-server
```

### View Logs

```bash
# Follow logs in real-time
sudo journalctl -u veryslip-server -f

# View last 100 lines
sudo journalctl -u veryslip-server -n 100
```

### Check Status

```bash
sudo systemctl status veryslip-server
```

## Multiple Domains Setup

For better performance, use 3-5 domains in comma-separated format.

### During Installation

When asked for domains, enter them separated by commas:

```
Enter domain(s) (comma-separated): t1.example.com, t2.example.com, t3.example.com, t4.example.com, t5.example.com
```

The script will automatically configure all domains.

### Benefits of Multiple Domains

- **2-3x better throughput**: Distributes DNS queries across multiple domains
- **Better reliability**: If one domain has issues, others continue working
- **Load balancing**: Automatic distribution of traffic

### Recommended Setup

- **Basic**: 1 domain (works fine)
- **Good**: 3 domains (2x performance)
- **Best**: 5 domains (3x performance)

### Example Configurations

**Single domain:**
```
tunnel.example.com
```

**Three domains:**
```
t1.example.com, t2.example.com, t3.example.com
```

**Five domains:**
```
t1.example.com, t2.example.com, t3.example.com, t4.example.com, t5.example.com
```

## Security Recommendations

### 1. Replace Self-Signed Certificates

The script generates self-signed certificates. For production, use Let's Encrypt:

```bash
# Install certbot
sudo apt install certbot

# Get certificate
sudo certbot certonly --standalone -d tunnel.example.com

# Copy to veryslip directory
sudo cp /etc/letsencrypt/live/tunnel.example.com/fullchain.pem /etc/veryslip-server/certs/cert.pem
sudo cp /etc/letsencrypt/live/tunnel.example.com/privkey.pem /etc/veryslip-server/certs/key.pem
sudo chown veryslip:veryslip /etc/veryslip-server/certs/*.pem

# Restart service
sudo systemctl restart veryslip-server
```

### 2. Restrict Metrics Port

By default, metrics are accessible on port 9090. Restrict to localhost:

```bash
# Using UFW
sudo ufw deny 9090/tcp
sudo ufw allow from 127.0.0.1 to any port 9090 proto tcp

# Using iptables
sudo iptables -A INPUT -p tcp --dport 9090 -s 127.0.0.1 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 9090 -j DROP
```

### 3. Enable Automatic Updates

```bash
# Ubuntu/Debian
sudo apt install unattended-upgrades
sudo dpkg-reconfigure -plow unattended-upgrades
```

## Troubleshooting

### Service Won't Start

```bash
# Check logs
sudo journalctl -u veryslip-server -n 50

# Check if port is in use
sudo netstat -tulpn | grep 8853

# Verify binary exists
ls -la /opt/veryslip-server/veryslip-server-bin
```

### DNS Not Working

```bash
# Check DNS records
dig @8.8.8.8 tunnel.example.com NS

# Test DNS query to server
dig @YOUR_SERVER_IP -p 8853 test.tunnel.example.com TXT

# Check firewall
sudo ufw status
sudo iptables -L -n | grep 8853
```

### Build Failed

```bash
# Check build log
cat /tmp/veryslip-build.log

# Manually rebuild
cd /opt/veryslip-server/veryslip-server
cargo build --release
```

## Updating

To update veryslip-server:

```bash
# Stop service
sudo systemctl stop veryslip-server

# Backup current binary
sudo cp /opt/veryslip-server/veryslip-server-bin /opt/veryslip-server/veryslip-server-bin.backup

# Pull latest code
cd /opt/veryslip-server/veryslip-server
git pull

# Rebuild
cargo build --release
sudo cp target/release/slipstream-server /opt/veryslip-server/veryslip-server-bin

# Start service
sudo systemctl start veryslip-server
```

## Uninstalling

To completely remove veryslip-server:

```bash
# Stop and disable service
sudo systemctl stop veryslip-server
sudo systemctl disable veryslip-server

# Remove files
sudo rm -rf /opt/veryslip-server
sudo rm -rf /etc/veryslip-server
sudo rm /etc/systemd/system/veryslip-server.service

# Remove user
sudo userdel veryslip

# Reload systemd
sudo systemctl daemon-reload
```

## Getting Help

- **GitHub Issues**: https://github.com/yourusername/veryslip/issues
- **Documentation**: https://github.com/yourusername/veryslip/wiki
- **Logs**: `sudo journalctl -u veryslip-server -f`

## Advanced Configuration

### Custom Ports

Edit the service file to change ports:

```bash
sudo nano /etc/systemd/system/veryslip-server.service

# Change --dns-listen-port or --metrics-port
# Then reload and restart:
sudo systemctl daemon-reload
sudo systemctl restart veryslip-server
```

### Compression Level

Default is level 5. To change:

```bash
# Edit service file
sudo nano /etc/systemd/system/veryslip-server.service

# Change --compression-level (1-9)
# Higher = better compression but more CPU
# Then reload and restart
```

### Connection Limits

```bash
# Edit service file
sudo nano /etc/systemd/system/veryslip-server.service

# Change --max-connections (default: 256)
# Then reload and restart
```

## Performance Tips

1. **Use multiple domains** (3-5) for best performance
2. **Use compression level 5-7** (balance between speed and compression)
3. **Increase max connections** if you have many clients
4. **Use SSD storage** for better I/O performance
5. **Enable BBR congestion control** on your VPS

## License

Apache-2.0 (inherited from slipstream-rust)
