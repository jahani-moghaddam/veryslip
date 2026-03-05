# VerySlip Repository Structure

This document describes the repository structure and what gets uploaded to GitHub.

## Repository Layout

```
veryslip/                              # Main repository
├── README.md                          # Main documentation
├── INSTALL.md                         # Complete installation guide
├── .gitignore                         # Git ignore rules
├── veryslip-server-deploy.sh          # One-click server deployment script
│
├── veryslip-client/                   # Client application
│   ├── Cargo.toml                     # Rust dependencies
│   ├── README.md                      # Client documentation
│   ├── src/                           # Source code
│   │   ├── main.rs                    # Entry point
│   │   ├── config/                    # Configuration module
│   │   ├── proxy/                     # HTTP proxy
│   │   ├── query/                     # DNS query engine
│   │   ├── cache/                     # Caching layer
│   │   ├── compression/               # Compression module
│   │   ├── filter/                    # Ad blocking
│   │   ├── load_balancer/             # Multi-domain load balancing
│   │   └── metrics/                   # Prometheus metrics
│   └── tests/                         # Test suite (139 tests)
│
└── veryslip-server/                   # Server application
    ├── Cargo.toml                     # Workspace configuration
    ├── README.md                      # Server documentation
    └── crates/                        # Rust crates
        ├── slipstream-server/         # Main server crate
        │   ├── src/
        │   │   ├── main.rs            # Entry point
        │   │   ├── server.rs          # Server logic
        │   │   ├── compression.rs     # Zstd compression
        │   │   ├── batch.rs           # Batch processing
        │   │   ├── metrics.rs         # Metrics collection
        │   │   └── metrics_server.rs  # Prometheus endpoint
        │   └── tests/                 # Test suite
        ├── slipstream-core/           # Core utilities
        ├── slipstream-dns/            # DNS protocol
        └── slipstream-ffi/            # C FFI for picoquic
```

## What Gets Uploaded to GitHub

### Included Files
- ✅ `README.md` - Main documentation
- ✅ `INSTALL.md` - Installation guide
- ✅ `.gitignore` - Git ignore rules
- ✅ `veryslip-server-deploy.sh` - Deployment script
- ✅ `veryslip-client/` - Complete client source code
- ✅ `veryslip-server/` - Complete server source code

### Excluded Files (via .gitignore)
- ❌ `target/` - Build artifacts
- ❌ `Cargo.lock` - Lock files
- ❌ `.kiro/` - Internal development specs
- ❌ `slipstream-rust-main/` - Upstream source (not needed)
- ❌ `patches/` - Already applied patches
- ❌ `build-docker.sh`, `Dockerfile.*` - Build scripts
- ❌ `slipstream-rust-deploy.sh`, `veryslip-deploy.sh` - Old scripts
- ❌ `*.log`, `*.tmp` - Temporary files
- ❌ `*.pem`, `*.key` - Certificates and keys
- ❌ `config.toml` - User configuration files

## GitHub Repository URL

```
https://github.com/jahani-moghaddam/veryslip
```

## Installation from GitHub

Users can install the server directly from GitHub:

```bash
# One-click installation
sudo bash <(wget -qO- https://raw.githubusercontent.com/jahani-moghaddam/veryslip/main/veryslip-server-deploy.sh)
```

The deployment script will:
1. Clone the repository (or use local files)
2. Build the server from source
3. Configure and start the service

## Building from Source

### Client
```bash
git clone https://github.com/jahani-moghaddam/veryslip.git
cd veryslip/veryslip-client
cargo build --release
```

### Server
```bash
git clone https://github.com/jahani-moghaddam/veryslip.git
cd veryslip/veryslip-server
cargo build --release
```

## Repository Size

Approximate sizes:
- Client source: ~500 KB
- Server source: ~800 KB
- Documentation: ~50 KB
- Total (without build artifacts): ~1.5 MB

Build artifacts (`target/` directories) are excluded via `.gitignore` and not uploaded to GitHub.

## Branches

- `main` - Stable production-ready code
- Feature branches can be created for development

## Deployment Script Location

The deployment script is at the repository root:
```
https://raw.githubusercontent.com/jahani-moghaddam/veryslip/main/veryslip-server-deploy.sh
```

This allows users to install with a single wget command.

## Documentation Structure

1. **README.md** (root) - Overview, quick start, features
2. **INSTALL.md** (root) - Detailed installation guide for server
3. **veryslip-client/README.md** - Client-specific documentation
4. **veryslip-server/README.md** - Server-specific documentation

## Git Commands

### Initial Setup
```bash
# Initialize and push
bash git-setup.sh --push
```

### Manual Setup
```bash
# Initialize repository
git init

# Add remote
git remote add origin https://github.com/jahani-moghaddam/veryslip.git

# Stage files
git add .gitignore README.md INSTALL.md veryslip-server-deploy.sh
git add veryslip-client/ veryslip-server/

# Commit
git commit -m "Initial commit: VerySlip DNS tunnel"

# Push
git branch -M main
git push -u origin main
```

### Update Repository
```bash
# Stage changes
git add .

# Commit
git commit -m "Update: description of changes"

# Push
git push origin main
```

## Notes

- The repository is public and accessible to anyone
- Build artifacts are generated locally and not committed
- User configuration files (config.toml) are excluded
- Certificates and keys are excluded for security
- The deployment script downloads and builds from source automatically
