#!/bin/bash
#
# VerySlip Server - One-Click Deployment Script
# 
# This script automatically installs and configures veryslip-server.
# Just run: sudo bash veryslip-server-deploy.sh
#

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

# Welcome banner
clear
echo -e "${BLUE}"
echo "========================================="
echo "  VerySlip Server - Easy Setup"
echo "========================================="
echo -e "${NC}"
echo ""
echo "This script will automatically:"
echo "  ✓ Install all dependencies"
echo "  ✓ Build the server"
echo "  ✓ Generate TLS certificates"
echo "  ✓ Configure firewall"
echo "  ✓ Set up systemd service"
echo "  ✓ Start the server"
echo ""
echo -e "${YELLOW}You only need to provide:${NC}"
echo "  1. Your domain name"
echo "  2. Tunnel mode (SOCKS/SSH/Shadowsocks)"
echo ""
read -p "Press Enter to continue or Ctrl+C to cancel..."
echo ""

# Ask for required information
log_step "Configuration"
echo ""

# Ask for domain(s)
echo "Enter your domain(s) for the DNS tunnel."
echo ""
echo "Examples:"
echo "  Single domain:    tunnel.example.com"
echo "  Multiple domains: t1.example.com, t2.example.com, t3.example.com"
echo ""
echo "Multiple domains improve performance (2-3x faster with 3-5 domains)."
echo ""
while true; do
    read -p "Enter domain(s) (comma-separated): " DOMAIN_INPUT
    if [[ -n "$DOMAIN_INPUT" ]]; then
        # Split by comma and trim whitespace
        IFS=',' read -ra DOMAINS <<< "$DOMAIN_INPUT"
        # Trim whitespace from each domain
        for i in "${!DOMAINS[@]}"; do
            DOMAINS[$i]=$(echo "${DOMAINS[$i]}" | xargs)
        done
        
        if [ ${#DOMAINS[@]} -gt 0 ]; then
            break
        fi
    fi
    echo -e "${RED}Please enter at least one domain.${NC}"
done

# Store first domain for certificate generation
DOMAIN="${DOMAINS[0]}"

# Ask for tunnel mode
echo ""
echo "Select tunnel mode:"
echo "  1) SOCKS proxy (recommended)"
echo "  2) SSH mode"
echo "  3) Shadowsocks"
echo ""
while true; do
    read -p "Enter choice (1, 2, or 3): " TUNNEL_MODE_CHOICE
    case $TUNNEL_MODE_CHOICE in
        1)
            TUNNEL_MODE="socks"
            TARGET_ADDRESS="127.0.0.1:1080"
            break
            ;;
        2)
            TUNNEL_MODE="ssh"
            TARGET_ADDRESS="127.0.0.1:22"
            break
            ;;
        3)
            TUNNEL_MODE="shadowsocks"
            TARGET_ADDRESS="127.0.0.1:8388"
            break
            ;;
        *)
            echo -e "${RED}Invalid choice. Please enter 1, 2, or 3${NC}"
            ;;
    esac
done

# Set optimal defaults (no need to ask user)
DNS_PORT="8853"
METRICS_PORT="9090"
COMPRESSION_LEVEL="5"
MAX_CONNECTIONS="256"
IDLE_TIMEOUT="60"

# Installation paths (automatic)
INSTALL_DIR="/opt/veryslip-server"
CERT_DIR="/etc/veryslip-server/certs"
SERVICE_USER="veryslip"
SERVICE_FILE="/etc/systemd/system/veryslip-server.service"

# Get server IP
SERVER_IP=$(hostname -I | awk '{print $1}')

# Show DNS setup instructions
clear
echo -e "${BLUE}"
echo "========================================="
echo "  DNS Configuration Required"
echo "========================================="
echo -e "${NC}"
echo ""
echo -e "${YELLOW}IMPORTANT: Before continuing, you must configure DNS records!${NC}"
echo ""
echo "Your server IP: ${GREEN}$SERVER_IP${NC}"
echo ""
echo "For each domain, add these DNS records:"
echo ""

for i in "${!DOMAINS[@]}"; do
    domain="${DOMAINS[$i]}"
    echo -e "${BLUE}Domain $(($i + 1)): $domain${NC}"
    echo "  1. Add A record:"
    echo "     ${GREEN}ns.$domain${NC}  →  ${GREEN}$SERVER_IP${NC}"
    echo ""
    echo "  2. Add NS record:"
    echo "     ${GREEN}$domain${NC}  →  ${GREEN}ns.$domain${NC}"
    echo ""
done

echo -e "${YELLOW}Example for Cloudflare/other DNS providers:${NC}"
echo "  Type: A     | Name: ns      | Content: $SERVER_IP"
echo "  Type: NS    | Name: @       | Content: ns.${DOMAINS[0]}"
echo ""
echo -e "${YELLOW}Note:${NC} DNS propagation can take 5-60 minutes."
echo ""
echo "Configuration summary:"
echo "  Domains: ${#DOMAINS[@]} configured"
for domain in "${DOMAINS[@]}"; do
    echo "    - $domain"
done
echo "  Tunnel Mode: $TUNNEL_MODE"
echo "  Target: $TARGET_ADDRESS (automatic based on mode)"
echo "  DNS Port: $DNS_PORT (automatic)"
echo "  Metrics Port: $METRICS_PORT (automatic)"
echo ""
read -p "Have you configured DNS records? Continue installation? (y/N): " CONTINUE_INSTALL

if [[ ! "$CONTINUE_INSTALL" =~ ^[Yy]$ ]]; then
    echo ""
    log_warn "Installation cancelled. Configure DNS records and run this script again."
    exit 0
fi

echo ""

# Check root
check_root() {
    if [ "$EUID" -ne 0 ]; then
        log_error "This script must be run as root (use sudo)"
        exit 1
    fi
}

log_step "Checking permissions..."
check_root

detect_os() {
    log_step "Detecting operating system..."
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        OS=$ID
        VER=$VERSION_ID
    else
        log_error "Cannot detect OS. /etc/os-release not found."
        exit 1
    fi
    log_info "Detected: $OS $VER"
}

install_dependencies() {
    log_step "Installing dependencies (this may take a few minutes)..."
    
    case $OS in
        ubuntu|debian)
            export DEBIAN_FRONTEND=noninteractive
            apt-get update -qq
            apt-get install -y -qq curl build-essential cmake pkg-config libssl-dev git > /dev/null 2>&1
            ;;
        centos|rhel|fedora)
            yum install -y -q curl gcc gcc-c++ make cmake pkg-config openssl-devel git > /dev/null 2>&1
            ;;
        *)
            log_error "Unsupported OS: $OS"
            exit 1
            ;;
    esac
    
    log_info "Dependencies installed"
}

install_rust() {
    log_step "Installing Rust compiler..."
    
    # Check if rustc exists for root
    if command -v rustc &> /dev/null; then
        log_info "Rust already installed ($(rustc --version))"
        return
    fi
    
    # Install Rust for root user
    export RUSTUP_HOME=/root/.rustup
    export CARGO_HOME=/root/.cargo
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y > /dev/null 2>&1
    
    # Source cargo env
    source /root/.cargo/env
    
    log_info "Rust installed ($(rustc --version))"
}

clone_and_build() {
    log_step "Downloading and building veryslip-server (5-10 minutes, please wait)..."
    
    # Create installation directory
    mkdir -p "$INSTALL_DIR"
    
    # Check if source already exists locally
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    
    if [ -d "$SCRIPT_DIR/veryslip-server" ]; then
        log_info "Using local veryslip-server directory"
        SOURCE_DIR="$SCRIPT_DIR/veryslip-server"
        cp -r "$SOURCE_DIR" "$INSTALL_DIR/" > /dev/null 2>&1
    elif [ -d "./veryslip-server" ]; then
        log_info "Using local veryslip-server directory"
        SOURCE_DIR="./veryslip-server"
        cp -r "$SOURCE_DIR" "$INSTALL_DIR/" > /dev/null 2>&1
    else
        # Clone from GitHub directly to final location
        log_info "Cloning from GitHub..."
        cd "$INSTALL_DIR"
        if ! git clone --depth 1 https://github.com/jahani-moghaddam/veryslip.git veryslip-temp > /dev/null 2>&1; then
            log_error "Failed to clone repository from GitHub"
            log_error "Please check your internet connection"
            exit 1
        fi
        
        # Move only veryslip-server with git metadata
        mv veryslip-temp/veryslip-server "$INSTALL_DIR/veryslip-server"
        rm -rf veryslip-temp
        log_info "Repository cloned successfully"
    fi
    
    cd "$INSTALL_DIR/veryslip-server"
    
    # Ensure picoquic submodule exists
    if [ ! -f vendor/picoquic/CMakeLists.txt ]; then
        log_info "Cloning picoquic submodule..."
        mkdir -p vendor
        if ! git clone --depth 1 https://github.com/Mygod/slipstream-picoquic vendor/picoquic > /dev/null 2>&1; then
            log_error "Failed to clone picoquic submodule"
            exit 1
        fi
        log_info "Picoquic submodule cloned successfully"
    else
        log_info "Picoquic submodule already exists"
    fi
    
    # Fix ownership and permissions BEFORE building
    chown -R root:root "$INSTALL_DIR/veryslip-server"
    find "$INSTALL_DIR/veryslip-server" -type d -exec chmod 755 {} \;
    find "$INSTALL_DIR/veryslip-server" -type f -exec chmod 644 {} \;
    
    # Make scripts executable
    if [ -d "$INSTALL_DIR/veryslip-server/scripts" ]; then
        find "$INSTALL_DIR/veryslip-server/scripts" -type f -name "*.sh" -exec chmod 755 {} \;
    fi
    
    # Ensure cargo is in PATH and using root's cargo
    export RUSTUP_HOME=/root/.rustup
    export CARGO_HOME=/root/.cargo
    export PATH="/root/.cargo/bin:$PATH"
    source /root/.cargo/env 2>/dev/null || true
    
    # Set umask to ensure new files are writable
    umask 022
    
    # Pre-create .picoquic-build directory with correct permissions
    # This is where the build script will compile picoquic
    mkdir -p "$INSTALL_DIR/veryslip-server/.picoquic-build"
    chown root:root "$INSTALL_DIR/veryslip-server/.picoquic-build"
    chmod 755 "$INSTALL_DIR/veryslip-server/.picoquic-build"
    
    # Set environment variables for picoquic build
    export PICOQUIC_DIR="$INSTALL_DIR/veryslip-server/vendor/picoquic"
    export PICOQUIC_BUILD_DIR="$INSTALL_DIR/veryslip-server/.picoquic-build"
    export PICOQUIC_AUTO_BUILD=1
    
    # Verify cmake is installed (required for picoquic build)
    if ! command -v cmake &> /dev/null; then
        log_error "cmake is required but not installed"
        log_error "This should have been installed with dependencies"
        exit 1
    fi
    
    # Pre-build picoquic manually to avoid permission issues during cargo build
    log_info "Pre-building picoquic dependencies..."
    if ! bash "$INSTALL_DIR/veryslip-server/scripts/build_picoquic.sh" > /tmp/picoquic-build.log 2>&1; then
        log_error "Picoquic build failed! Check /tmp/picoquic-build.log"
        tail -20 /tmp/picoquic-build.log
        exit 1
    fi
    
    # Build with progress indicator
    echo -n "  Building Rust components"
    
    # Add verbose logging for debugging
    export RUST_BACKTRACE=1
    
    if ! cargo build --release > /tmp/veryslip-build.log 2>&1; then
        echo ""
        log_error "Build failed! Check /tmp/veryslip-build.log for details"
        echo ""
        echo "Last 30 lines of build log:"
        tail -30 /tmp/veryslip-build.log
        exit 1
    fi
    echo " ✓"
    
    # Copy binary
    cp target/release/slipstream-server "$INSTALL_DIR/veryslip-server-bin"
    chmod +x "$INSTALL_DIR/veryslip-server-bin"
    
    log_info "Build completed"
}

generate_certificates() {
    log_step "Generating TLS certificates..."
    
    mkdir -p "$CERT_DIR"
    
    if [ -f "$CERT_DIR/cert.pem" ] && [ -f "$CERT_DIR/key.pem" ]; then
        log_info "Certificates already exist, skipping"
        return
    fi
    
    # Generate self-signed certificate silently
    openssl ecparam -genkey -name prime256v1 -out "$CERT_DIR/key.pem" 2>/dev/null
    openssl req -new -x509 -key "$CERT_DIR/key.pem" -out "$CERT_DIR/cert.pem" -days 3650 \
        -subj "/C=US/ST=State/L=City/O=Organization/CN=$DOMAIN" 2>/dev/null
    
    chmod 600 "$CERT_DIR/key.pem"
    chmod 644 "$CERT_DIR/cert.pem"
    
    log_info "Certificates generated"
    log_warn "Using self-signed certificates. Replace with Let's Encrypt for production!"
}

create_service_user() {
    log_step "Creating service user..."
    
    if id "$SERVICE_USER" &>/dev/null; then
        log_info "User already exists"
    else
        useradd --system --no-create-home --shell /bin/false "$SERVICE_USER" 2>/dev/null
        log_info "User created"
    fi
    
    chown -R "$SERVICE_USER:$SERVICE_USER" "$CERT_DIR"
}

create_systemd_service() {
    log_step "Configuring systemd service..."
    
    # Build domain arguments
    DOMAIN_ARGS=""
    for domain in "${DOMAINS[@]}"; do
        DOMAIN_ARGS="$DOMAIN_ARGS --domain $domain"
    done
    
    cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=VerySlip DNS Tunnel Server
After=network.target
Documentation=https://github.com/yourusername/veryslip-server

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_USER
ExecStart=$INSTALL_DIR/veryslip-server-bin \\
    --dns-listen-host :: \\
    --dns-listen-port $DNS_PORT \\
    --target-address $TARGET_ADDRESS$DOMAIN_ARGS \\
    --cert $CERT_DIR/cert.pem \\
    --key $CERT_DIR/key.pem \\
    --compression-level $COMPRESSION_LEVEL \\
    --metrics-port $METRICS_PORT \\
    --max-connections $MAX_CONNECTIONS \\
    --idle-timeout-seconds $IDLE_TIMEOUT
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal
SyslogIdentifier=veryslip-server

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=$CERT_DIR

# Resource limits
LimitNOFILE=65536
LimitNPROC=512

[Install]
WantedBy=multi-user.target
EOF
    
    systemctl daemon-reload 2>/dev/null
    log_info "Service configured"
}

configure_firewall() {
    log_step "Configuring firewall..."
    
    if command -v ufw &> /dev/null; then
        ufw allow "$DNS_PORT/udp" comment "VerySlip DNS" > /dev/null 2>&1 || true
        ufw allow "$METRICS_PORT/tcp" comment "VerySlip Metrics" > /dev/null 2>&1 || true
        log_info "UFW rules added"
    elif command -v firewall-cmd &> /dev/null; then
        firewall-cmd --permanent --add-port="$DNS_PORT/udp" > /dev/null 2>&1 || true
        firewall-cmd --permanent --add-port="$METRICS_PORT/tcp" > /dev/null 2>&1 || true
        firewall-cmd --reload > /dev/null 2>&1 || true
        log_info "firewalld rules added"
    else
        log_warn "No firewall detected, skipping"
    fi
}

tune_system() {
    log_step "Optimizing system parameters..."
    
    # Backup sysctl.conf
    cp /etc/sysctl.conf /etc/sysctl.conf.backup 2>/dev/null || true
    
    # Add tuning parameters
    cat >> /etc/sysctl.conf <<EOF

# VerySlip Server Tuning (added by veryslip-server-deploy.sh)
net.core.rmem_max = 26214400
net.core.rmem_default = 26214400
net.core.wmem_max = 26214400
net.core.wmem_default = 26214400
net.core.netdev_max_backlog = 5000
net.ipv4.udp_mem = 10240 87380 26214400
EOF
    
    sysctl -p > /dev/null 2>&1
    log_info "System optimized"
}

setup_tunnel_backend() {
    log_step "Setting up tunnel backend ($TUNNEL_MODE)..."
    
    case $TUNNEL_MODE in
        socks)
            # Install and configure Dante SOCKS server
            log_info "Installing Dante SOCKS server..."
            case $OS in
                ubuntu|debian)
                    export DEBIAN_FRONTEND=noninteractive
                    apt-get install -y -qq dante-server > /dev/null 2>&1
                    ;;
                centos|rhel|fedora)
                    yum install -y -q dante-server > /dev/null 2>&1
                    ;;
            esac
            
            # Get primary network interface
            PRIMARY_INTERFACE=$(ip route | grep default | awk '{print $5}' | head -n1)
            if [ -z "$PRIMARY_INTERFACE" ]; then
                PRIMARY_INTERFACE="eth0"
                log_warn "Could not detect network interface, using eth0"
            fi
            
            # Create Dante configuration
            cat > /etc/danted.conf <<EOF
# Dante SOCKS server configuration (auto-generated by veryslip-server-deploy.sh)
logoutput: syslog

# Listen on localhost only (veryslip-server will forward to this)
internal: 127.0.0.1 port = 1080

# External interface for outgoing connections
external: $PRIMARY_INTERFACE

# No authentication required (only accessible from localhost)
clientmethod: none
socksmethod: none

# Run as unprivileged user
user.privileged: root
user.unprivileged: nobody

# Allow connections from localhost only
client pass {
    from: 127.0.0.1/8 to: 0.0.0.0/0
}

socks pass {
    from: 127.0.0.1/8 to: 0.0.0.0/0
}
EOF
            
            # Start Dante service
            systemctl enable danted > /dev/null 2>&1
            systemctl restart danted
            
            sleep 2
            
            if systemctl is-active --quiet danted; then
                log_info "Dante SOCKS server started on 127.0.0.1:1080"
            else
                log_error "Dante SOCKS server failed to start"
                echo ""
                echo "Check logs with: journalctl -u danted -n 50"
                exit 1
            fi
            ;;
            
        ssh)
            # SSH should already be running
            if systemctl is-active --quiet sshd || systemctl is-active --quiet ssh; then
                log_info "SSH server is already running on port 22"
            else
                log_warn "SSH server not running. Install with: apt install openssh-server"
            fi
            ;;
            
        shadowsocks)
            log_warn "Shadowsocks not installed automatically"
            log_warn "Install manually: apt install shadowsocks-libev"
            log_warn "Configure: /etc/shadowsocks-libev/config.json"
            ;;
    esac
}

start_service() {
    log_step "Starting veryslip-server..."
    
    systemctl enable veryslip-server > /dev/null 2>&1
    systemctl start veryslip-server
    
    sleep 3
    
    if systemctl is-active --quiet veryslip-server; then
        log_info "Service started successfully"
    else
        log_error "Service failed to start"
        echo ""
        echo "Check logs with: journalctl -u veryslip-server -n 50"
        exit 1
    fi
}

health_check() {
    log_step "Performing health check..."
    
    if ! systemctl is-active --quiet veryslip-server; then
        log_error "Service is not running"
        return 1
    fi
    
    # Check metrics endpoint
    if curl -s "http://localhost:$METRICS_PORT/health" > /dev/null 2>&1; then
        log_info "Metrics endpoint: OK"
    else
        log_warn "Metrics endpoint not responding (may take a moment to start)"
    fi
    
    log_info "Health check completed"
}

print_summary() {
    clear
    echo -e "${GREEN}"
    echo "========================================="
    echo "  ✓ VerySlip Server Installed!"
    echo "========================================="
    echo -e "${NC}"
    echo ""
    echo -e "${BLUE}Your Configuration:${NC}"
    echo "  Domains: ${#DOMAINS[@]} configured"
    for domain in "${DOMAINS[@]}"; do
        echo "    - $domain"
    done
    echo "  Tunnel Mode: $TUNNEL_MODE"
    echo "  Target: $TARGET_ADDRESS"
    
    # Show backend status
    case $TUNNEL_MODE in
        socks)
            if systemctl is-active --quiet danted; then
                echo "  SOCKS Backend: ✓ Running (Dante on 127.0.0.1:1080)"
            else
                echo "  SOCKS Backend: ✗ Not running"
            fi
            ;;
        ssh)
            if systemctl is-active --quiet sshd || systemctl is-active --quiet ssh; then
                echo "  SSH Backend: ✓ Running (port 22)"
            else
                echo "  SSH Backend: ✗ Not running (install openssh-server)"
            fi
            ;;
        shadowsocks)
            echo "  Shadowsocks Backend: Manual setup required"
            ;;
    esac
    
    echo "  DNS Port: $DNS_PORT"
    echo "  Metrics: http://localhost:$METRICS_PORT/metrics"
    echo ""
    echo -e "${BLUE}Service Management:${NC}"
    echo "  View logs:    journalctl -u veryslip-server -f"
    echo "  Restart:      systemctl restart veryslip-server"
    echo "  Stop:         systemctl stop veryslip-server"
    echo "  Status:       systemctl status veryslip-server"
    echo ""
    
    if [ "$TUNNEL_MODE" = "socks" ]; then
        echo -e "${BLUE}SOCKS Server Management:${NC}"
        echo "  View logs:    journalctl -u danted -f"
        echo "  Restart:      systemctl restart danted"
        echo "  Status:       systemctl status danted"
        echo ""
    fi
    
    echo -e "${BLUE}Next Steps:${NC}"
    
    if [ "$TUNNEL_MODE" = "socks" ]; then
        echo "  1. ✓ SOCKS server is running and ready"
    elif [ "$TUNNEL_MODE" = "ssh" ]; then
        echo "  1. Ensure SSH server is running: systemctl status sshd"
    else
        echo "  1. Install and configure Shadowsocks on 127.0.0.1:8388"
        echo "     apt install shadowsocks-libev"
        echo "     Configure: /etc/shadowsocks-libev/config.json"
    fi
    
    echo ""
    echo "  2. Verify DNS records are propagated:"
    echo "     dig @8.8.8.8 ${DOMAINS[0]} NS"
    echo ""
    echo "  3. Replace self-signed certificate (optional)"
    echo "     Certificates: $CERT_DIR/"
    echo ""
    echo "  4. Configure your veryslip-client to use:"
    for domain in "${DOMAINS[@]}"; do
        echo "     Domain: $domain"
    done
    echo "     Resolvers: 8.8.8.8:53, 1.1.1.1:53"
    echo ""
    echo -e "${YELLOW}Important:${NC} For production, replace self-signed certificates with Let's Encrypt!"
    echo ""
}

# Main deployment flow
main() {
    detect_os
    install_dependencies
    install_rust
    clone_and_build
    generate_certificates
    create_service_user
    create_systemd_service
    configure_firewall
    tune_system
    setup_tunnel_backend
    start_service
    health_check
    print_summary
}

# Run main function
main
