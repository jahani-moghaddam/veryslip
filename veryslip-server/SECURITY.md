# VerySlip Server Security Hardening Guide

## Overview

This guide provides comprehensive security recommendations for deploying and operating VerySlip Server in production environments. Follow these guidelines to minimize attack surface and protect your infrastructure.

## Table of Contents

1. [TLS/Certificate Management](#tlscertificate-management)
2. [Firewall Configuration](#firewall-configuration)
3. [System Hardening](#system-hardening)
4. [Access Control](#access-control)
5. [Monitoring and Logging](#monitoring-and-logging)
6. [Rate Limiting](#rate-limiting)
7. [Regular Maintenance](#regular-maintenance)

## TLS/Certificate Management

### Use Proper Certificates

**Never use self-signed certificates in production.** Use certificates from a trusted CA like Let's Encrypt.

#### Let's Encrypt Setup

```bash
# Install certbot
sudo apt-get install certbot  # Ubuntu/Debian
sudo yum install certbot      # CentOS/RHEL

# Obtain certificate (standalone mode)
sudo certbot certonly --standalone -d yourdomain.com

# Copy certificates to veryslip directory
sudo cp /etc/letsencrypt/live/yourdomain.com/fullchain.pem /etc/veryslip-server/certs/cert.pem
sudo cp /etc/letsencrypt/live/yourdomain.com/privkey.pem /etc/veryslip-server/certs/key.pem
sudo chown veryslip:veryslip /etc/veryslip-server/certs/*.pem
sudo chmod 600 /etc/veryslip-server/certs/key.pem
sudo chmod 644 /etc/veryslip-server/certs/cert.pem

# Restart service
sudo systemctl restart veryslip-server
```

#### Automatic Certificate Renewal

Create a renewal hook at `/etc/letsencrypt/renewal-hooks/deploy/veryslip-renew.sh`:

```bash
#!/bin/bash
cp /etc/letsencrypt/live/yourdomain.com/fullchain.pem /etc/veryslip-server/certs/cert.pem
cp /etc/letsencrypt/live/yourdomain.com/privkey.pem /etc/veryslip-server/certs/key.pem
chown veryslip:veryslip /etc/veryslip-server/certs/*.pem
chmod 600 /etc/veryslip-server/certs/key.pem
systemctl restart veryslip-server
```

Make it executable:
```bash
sudo chmod +x /etc/letsencrypt/renewal-hooks/deploy/veryslip-renew.sh
```

Test renewal:
```bash
sudo certbot renew --dry-run
```

### Certificate Monitoring

Monitor certificate expiration:

```bash
# Check expiration date
openssl x509 -in /etc/veryslip-server/certs/cert.pem -enddate -noout

# Set up monitoring alert (30 days before expiration)
# Add to crontab:
0 0 * * * /usr/local/bin/check-cert-expiry.sh
```

Create `/usr/local/bin/check-cert-expiry.sh`:

```bash
#!/bin/bash
CERT="/etc/veryslip-server/certs/cert.pem"
DAYS_WARNING=30

expiry_date=$(openssl x509 -in "$CERT" -enddate -noout | cut -d= -f2)
expiry_epoch=$(date -d "$expiry_date" +%s)
current_epoch=$(date +%s)
days_left=$(( ($expiry_epoch - $current_epoch) / 86400 ))

if [ $days_left -lt $DAYS_WARNING ]; then
    echo "WARNING: Certificate expires in $days_left days!" | mail -s "VerySlip Certificate Expiry Warning" admin@example.com
fi
```

## Firewall Configuration

### Principle: Deny All, Allow Specific

#### UFW (Ubuntu/Debian)

```bash
# Reset firewall
sudo ufw --force reset

# Default policies
sudo ufw default deny incoming
sudo ufw default allow outgoing

# Allow SSH (change port if using non-standard)
sudo ufw allow 22/tcp comment "SSH"

# Allow DNS tunnel port
sudo ufw allow 8853/udp comment "VerySlip DNS"

# Restrict metrics to specific IPs only
# Option 1: Localhost only
sudo ufw allow from 127.0.0.1 to any port 9090 proto tcp comment "VerySlip Metrics (localhost)"

# Option 2: Specific monitoring server
sudo ufw allow from 10.0.0.5 to any port 9090 proto tcp comment "VerySlip Metrics (monitoring)"

# Option 3: Private network only
sudo ufw allow from 10.0.0.0/8 to any port 9090 proto tcp comment "VerySlip Metrics (private)"

# Enable firewall
sudo ufw enable

# Verify rules
sudo ufw status numbered
```

#### firewalld (CentOS/RHEL/Fedora)

```bash
# Set default zone to drop
sudo firewall-cmd --set-default-zone=drop

# Create custom zone for veryslip
sudo firewall-cmd --permanent --new-zone=veryslip
sudo firewall-cmd --permanent --zone=veryslip --add-port=8853/udp

# Restrict metrics
sudo firewall-cmd --permanent --zone=veryslip --add-rich-rule='
  rule family="ipv4"
  source address="10.0.0.0/8"
  port protocol="tcp" port="9090" accept'

# Allow SSH
sudo firewall-cmd --permanent --zone=public --add-service=ssh

# Reload
sudo firewall-cmd --reload

# Verify
sudo firewall-cmd --list-all-zones
```

### Port Knocking (Advanced)

For additional security, implement port knocking for SSH:

```bash
# Install knockd
sudo apt-get install knockd

# Configure /etc/knockd.conf
[options]
    UseSyslog

[openSSH]
    sequence    = 7000,8000,9000
    seq_timeout = 5
    command     = /sbin/iptables -A INPUT -s %IP% -p tcp --dport 22 -j ACCEPT
    tcpflags    = syn

[closeSSH]
    sequence    = 9000,8000,7000
    seq_timeout = 5
    command     = /sbin/iptables -D INPUT -s %IP% -p tcp --dport 22 -j ACCEPT
    tcpflags    = syn
```

## System Hardening

### SELinux (CentOS/RHEL/Fedora)

```bash
# Verify SELinux is enabled
sestatus

# Create custom policy for veryslip
sudo semanage port -a -t dns_port_t -p udp 8853
sudo semanage port -a -t http_port_t -p tcp 9090

# Allow veryslip to bind to ports
sudo setsebool -P nis_enabled 1

# Create custom module if needed
sudo audit2allow -a -M veryslip
sudo semodule -i veryslip.pp
```

### AppArmor (Ubuntu/Debian)

Create `/etc/apparmor.d/opt.veryslip-server.veryslip-server-bin`:

```
#include <tunables/global>

/opt/veryslip-server/veryslip-server-bin {
  #include <abstractions/base>
  #include <abstractions/nameservice>
  #include <abstractions/openssl>

  # Binary
  /opt/veryslip-server/veryslip-server-bin mr,

  # Certificates
  /etc/veryslip-server/certs/* r,

  # Network
  network inet dgram,
  network inet6 dgram,
  network inet stream,
  network inet6 stream,

  # Capabilities
  capability net_bind_service,
  capability setuid,
  capability setgid,

  # Deny everything else
  deny /proc/** w,
  deny /sys/** w,
}
```

Load the profile:
```bash
sudo apparmor_parser -r /etc/apparmor.d/opt.veryslip-server.veryslip-server-bin
```

### Systemd Hardening

Add to `/etc/systemd/system/veryslip-server.service`:

```ini
[Service]
# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
SystemCallFilter=@system-service
SystemCallErrorNumber=EPERM
SystemCallArchitectures=native

# Resource limits
LimitNOFILE=65536
LimitNPROC=512
LimitCORE=0
TasksMax=512

# Read-write paths
ReadWritePaths=/etc/veryslip-server/certs
```

Reload systemd:
```bash
sudo systemctl daemon-reload
sudo systemctl restart veryslip-server
```

## Access Control

### Service User Isolation

Ensure the service runs as a dedicated non-privileged user:

```bash
# Create user with no login shell
sudo useradd --system --no-create-home --shell /bin/false veryslip

# Set ownership
sudo chown -R veryslip:veryslip /etc/veryslip-server/certs
sudo chown veryslip:veryslip /opt/veryslip-server/veryslip-server-bin

# Verify
id veryslip
```

### File Permissions

```bash
# Binary
sudo chmod 755 /opt/veryslip-server/veryslip-server-bin

# Certificates
sudo chmod 600 /etc/veryslip-server/certs/key.pem
sudo chmod 644 /etc/veryslip-server/certs/cert.pem

# Directory
sudo chmod 750 /etc/veryslip-server/certs

# Verify
ls -la /etc/veryslip-server/certs/
```

### SSH Hardening

Edit `/etc/ssh/sshd_config`:

```
# Disable root login
PermitRootLogin no

# Use key-based authentication only
PasswordAuthentication no
PubkeyAuthentication yes

# Disable empty passwords
PermitEmptyPasswords no

# Limit users
AllowUsers yourusername

# Use strong ciphers
Ciphers chacha20-poly1305@openssh.com,aes256-gcm@openssh.com
MACs hmac-sha2-512-etm@openssh.com,hmac-sha2-256-etm@openssh.com
KexAlgorithms curve25519-sha256,curve25519-sha256@libssh.org

# Change default port (optional)
Port 2222
```

Restart SSH:
```bash
sudo systemctl restart sshd
```

## Monitoring and Logging

### Centralized Logging

Forward logs to a centralized logging server:

```bash
# Install rsyslog
sudo apt-get install rsyslog

# Configure /etc/rsyslog.d/veryslip.conf
:programname, isequal, "veryslip-server" @@logserver.example.com:514

# Restart rsyslog
sudo systemctl restart rsyslog
```

### Log Rotation

Create `/etc/logrotate.d/veryslip`:

```
/var/log/veryslip/*.log {
    daily
    rotate 30
    compress
    delaycompress
    notifempty
    create 0640 veryslip veryslip
    sharedscripts
    postrotate
        systemctl reload veryslip-server > /dev/null 2>&1 || true
    endscript
}
```

### Intrusion Detection

Install and configure fail2ban:

```bash
# Install
sudo apt-get install fail2ban

# Create /etc/fail2ban/filter.d/veryslip.conf
[Definition]
failregex = ^.*veryslip-server.*Failed.*from <HOST>.*$
ignoreregex =

# Create /etc/fail2ban/jail.d/veryslip.conf
[veryslip]
enabled = true
port = 8853
protocol = udp
filter = veryslip
logpath = /var/log/syslog
maxretry = 10
bantime = 3600
findtime = 600

# Restart fail2ban
sudo systemctl restart fail2ban
```

### Metrics Monitoring

Set up Prometheus alerts for suspicious activity:

```yaml
# prometheus-alerts.yml
groups:
  - name: veryslip
    rules:
      - alert: HighQueryRate
        expr: rate(veryslip_queries_total[5m]) > 1000
        for: 5m
        annotations:
          summary: "High query rate detected"
          
      - alert: HighConnectionCount
        expr: veryslip_active_connections > 500
        for: 5m
        annotations:
          summary: "Unusually high connection count"
          
      - alert: LowCompressionRatio
        expr: veryslip_compression_ratio < 0.3
        for: 10m
        annotations:
          summary: "Compression ratio is unusually low"
```

## Rate Limiting

### Application-Level Rate Limiting

Configure rate limiting in veryslip-server:

```bash
# Add to systemd service ExecStart
--rate-limit 100  # 100 queries per second per IP
```

### Kernel-Level Rate Limiting

Use iptables for additional protection:

```bash
# Limit new connections per IP
sudo iptables -A INPUT -p udp --dport 8853 -m state --state NEW -m recent --set
sudo iptables -A INPUT -p udp --dport 8853 -m state --state NEW -m recent --update --seconds 60 --hitcount 100 -j DROP

# Save rules
sudo iptables-save > /etc/iptables/rules.v4
```

### Connection Tracking

Tune conntrack for high connection counts:

```bash
# Add to /etc/sysctl.conf
net.netfilter.nf_conntrack_max = 262144
net.netfilter.nf_conntrack_tcp_timeout_established = 600
net.netfilter.nf_conntrack_udp_timeout = 60
net.netfilter.nf_conntrack_udp_timeout_stream = 120

# Apply
sudo sysctl -p
```

## Regular Maintenance

### Security Updates

```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get upgrade
sudo apt-get dist-upgrade

# CentOS/RHEL
sudo yum update
```

Set up automatic security updates:

```bash
# Ubuntu/Debian
sudo apt-get install unattended-upgrades
sudo dpkg-reconfigure -plow unattended-upgrades

# CentOS/RHEL
sudo yum install yum-cron
sudo systemctl enable yum-cron
sudo systemctl start yum-cron
```

### Audit Logs

Regularly review logs for suspicious activity:

```bash
# Check authentication logs
sudo grep -i "failed\|error" /var/log/auth.log

# Check veryslip logs
sudo journalctl -u veryslip-server --since "1 day ago" | grep -i "error\|fail"

# Check firewall logs
sudo grep -i "UFW BLOCK" /var/log/syslog
```

### Vulnerability Scanning

Run regular vulnerability scans:

```bash
# Install lynis
sudo apt-get install lynis

# Run audit
sudo lynis audit system

# Review report
sudo cat /var/log/lynis.log
```

### Backup Strategy

Regular backups of critical files:

```bash
#!/bin/bash
# /usr/local/bin/veryslip-backup.sh

BACKUP_DIR="/backup/veryslip"
DATE=$(date +%Y%m%d-%H%M%S)

mkdir -p "$BACKUP_DIR"

# Backup certificates
tar -czf "$BACKUP_DIR/certs-$DATE.tar.gz" /etc/veryslip-server/certs/

# Backup configuration
tar -czf "$BACKUP_DIR/config-$DATE.tar.gz" /etc/systemd/system/veryslip-server.service

# Keep only last 30 days
find "$BACKUP_DIR" -name "*.tar.gz" -mtime +30 -delete

# Upload to remote storage (optional)
# aws s3 cp "$BACKUP_DIR/certs-$DATE.tar.gz" s3://your-bucket/veryslip/
```

Add to crontab:
```bash
0 2 * * * /usr/local/bin/veryslip-backup.sh
```

## Incident Response

### Compromise Detection

Signs of potential compromise:
- Unusual spike in query rate
- Connections from unexpected geographic locations
- High CPU/memory usage
- Modified binaries or configuration files
- Unexpected network connections

### Response Procedure

1. **Isolate the server**:
   ```bash
   sudo systemctl stop veryslip-server
   sudo ufw deny 8853/udp
   ```

2. **Preserve evidence**:
   ```bash
   sudo tar -czf /tmp/forensics-$(date +%Y%m%d).tar.gz \
       /var/log/ \
       /etc/veryslip-server/ \
       /opt/veryslip-server/
   ```

3. **Analyze logs**:
   ```bash
   sudo journalctl -u veryslip-server --since "24 hours ago" > /tmp/veryslip-logs.txt
   ```

4. **Check for rootkits**:
   ```bash
   sudo apt-get install rkhunter chkrootkit
   sudo rkhunter --check
   sudo chkrootkit
   ```

5. **Restore from backup** if compromised

6. **Update and harden** before bringing back online

## Security Checklist

- [ ] Use proper TLS certificates (not self-signed)
- [ ] Configure automatic certificate renewal
- [ ] Implement strict firewall rules
- [ ] Restrict metrics endpoint access
- [ ] Enable SELinux or AppArmor
- [ ] Run service as non-privileged user
- [ ] Harden SSH configuration
- [ ] Set up centralized logging
- [ ] Configure log rotation
- [ ] Install intrusion detection (fail2ban)
- [ ] Enable rate limiting
- [ ] Set up monitoring and alerts
- [ ] Configure automatic security updates
- [ ] Implement regular backups
- [ ] Document incident response procedures
- [ ] Conduct regular security audits

## Additional Resources

- [CIS Benchmarks](https://www.cisecurity.org/cis-benchmarks/)
- [NIST Cybersecurity Framework](https://www.nist.gov/cyberframework)
- [OWASP Security Guidelines](https://owasp.org/)
- [Linux Security Hardening Guide](https://www.kernel.org/doc/html/latest/admin-guide/security.html)

## Support

For security issues, please report privately to: security@example.com

Do not disclose security vulnerabilities publicly until they have been addressed.
