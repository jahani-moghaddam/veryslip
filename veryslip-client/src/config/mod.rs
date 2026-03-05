use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Tunnel domains (required, 1-50 domains)
    pub domains: Vec<String>,
    
    /// DNS resolvers (required)
    pub resolvers: Vec<String>,
    
    #[serde(default)]
    pub proxy: ProxyConfig,
    
    #[serde(default)]
    pub compression: CompressionConfig,
    
    #[serde(default)]
    pub cache: CacheConfig,
    
    #[serde(default)]
    pub query: QueryConfig,
    
    #[serde(default)]
    pub doh: DohConfigSection,
    
    #[serde(default)]
    pub mtu: MtuConfig,
    
    #[serde(default)]
    pub load_balancer: LoadBalancerConfig,
    
    #[serde(default)]
    pub priority: PriorityConfig,
    
    #[serde(default)]
    pub connection: ConnectionConfig,
    
    #[serde(default)]
    pub buffer: BufferConfig,
    
    #[serde(default)]
    pub prefetch: PrefetchConfig,
    
    #[serde(default)]
    pub filter: FilterConfig,
    
    #[serde(default)]
    pub metrics: MetricsConfig,
    
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default = "default_proxy_port")]
    pub port: u16,
    
    #[serde(default)]
    pub auth_enabled: bool,
    
    #[serde(default)]
    pub username: Option<String>,
    
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    #[serde(default = "default_compression_level")]
    pub level: i32,
    
    #[serde(default = "default_true")]
    pub adaptive: bool,
    
    #[serde(default)]
    pub dictionary_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    #[serde(default = "default_cache_size")]
    pub max_memory_size: usize,
    
    #[serde(default)]
    pub disk_path: Option<PathBuf>,
    
    #[serde(default = "default_cache_ttl_html")]
    pub default_ttl_html_secs: u64,
    
    #[serde(default = "default_cache_ttl_css_js")]
    pub default_ttl_css_js_secs: u64,
    
    #[serde(default = "default_cache_ttl_images")]
    pub default_ttl_images_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryConfig {
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    
    #[serde(default = "default_max_in_flight")]
    pub max_in_flight: usize,
    
    #[serde(default = "default_batch_timeout_ms")]
    pub batch_timeout_ms: u64,
    
    #[serde(default = "default_batch_threshold")]
    pub batch_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DohConfigSection {
    #[serde(default)]
    pub enabled: bool,
    
    #[serde(default = "default_doh_endpoints")]
    pub endpoints: Vec<String>,
    
    #[serde(default = "default_doh_timeout_secs")]
    pub timeout_secs: u64,
    
    #[serde(default)]
    pub use_get_method: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtuConfig {
    #[serde(default = "default_min_mtu")]
    pub min_size: usize,
    
    #[serde(default = "default_max_mtu")]
    pub max_size: usize,
    
    #[serde(default = "default_probe_timeout_ms")]
    pub probe_timeout_ms: u64,
    
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadBalancerConfig {
    #[serde(default = "default_failure_timeout_secs")]
    pub failure_timeout_secs: u64,
    
    #[serde(default = "default_window_size_secs")]
    pub window_size_secs: u64,
    
    #[serde(default = "default_success_threshold")]
    pub success_threshold: f32,
    
    #[serde(default = "default_weight_reduction")]
    pub weight_reduction: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityConfig {
    #[serde(default = "default_bandwidth_weights")]
    pub bandwidth_weights: [f32; 4],
    
    #[serde(default = "default_starvation_timeout_secs")]
    pub starvation_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    
    #[serde(default = "default_max_streams")]
    pub max_streams_per_connection: usize,
    
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferConfig {
    #[serde(default = "default_initial_buffer_size")]
    pub initial_size: usize,
    
    #[serde(default = "default_max_buffer_size")]
    pub max_size: usize,
    
    #[serde(default = "default_buffer_capacity")]
    pub buffer_capacity: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    #[serde(default = "default_prefetch_queue_size")]
    pub max_queue_size: usize,
    
    #[serde(default = "default_resource_types")]
    pub resource_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    #[serde(default)]
    pub blocklist_path: Option<PathBuf>,
    
    #[serde(default)]
    pub whitelist: Vec<String>,
    
    #[serde(default)]
    pub update_enabled: bool,
    
    #[serde(default)]
    pub update_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    #[serde(default = "default_metrics_port")]
    pub http_port: u16,
    
    #[serde(default = "default_log_interval_secs")]
    pub log_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    
    #[serde(default = "default_log_format")]
    pub format: String,
    
    #[serde(default)]
    pub file_enabled: bool,
    
    #[serde(default)]
    pub file_path: Option<PathBuf>,
    
    #[serde(default = "default_file_max_size")]
    pub file_max_size: u64,
    
    #[serde(default = "default_file_max_count")]
    pub file_max_count: usize,
}

// Default value functions
fn default_proxy_port() -> u16 { 8080 }
fn default_compression_level() -> i32 { 3 }
fn default_cache_size() -> usize { 524_288_000 } // 500MB
fn default_cache_ttl_html() -> u64 { 3600 } // 1 hour
fn default_cache_ttl_css_js() -> u64 { 86400 } // 24 hours
fn default_cache_ttl_images() -> u64 { 604800 } // 7 days
fn default_concurrency() -> usize { 8 }
fn default_max_in_flight() -> usize { 1000 }
fn default_batch_timeout_ms() -> u64 { 5 }
fn default_batch_threshold() -> f32 { 0.8 }
fn default_doh_endpoints() -> Vec<String> {
    vec!["https://cloudflare-dns.com/dns-query".to_string()]
}
fn default_doh_timeout_secs() -> u64 { 5 }
fn default_min_mtu() -> usize { 900 }
fn default_max_mtu() -> usize { 1400 }
fn default_probe_timeout_ms() -> u64 { 2000 }
fn default_failure_threshold() -> f32 { 0.1 }
fn default_failure_timeout_secs() -> u64 { 60 }
fn default_window_size_secs() -> u64 { 300 }
fn default_success_threshold() -> f32 { 0.5 }
fn default_weight_reduction() -> f32 { 0.75 }
fn default_bandwidth_weights() -> [f32; 4] { [0.4, 0.3, 0.2, 0.1] }
fn default_starvation_timeout_secs() -> u64 { 30 }
fn default_max_connections() -> usize { 10 }
fn default_max_streams() -> usize { 100 }
fn default_idle_timeout_secs() -> u64 { 60 }
fn default_initial_buffer_size() -> usize { 100 }
fn default_max_buffer_size() -> usize { 10000 }
fn default_buffer_capacity() -> usize { 1400 }
fn default_prefetch_queue_size() -> usize { 50 }
fn default_resource_types() -> Vec<String> {
    vec!["stylesheet".to_string(), "script".to_string(), "image".to_string()]
}
fn default_metrics_port() -> u16 { 9091 }
fn default_log_interval_secs() -> u64 { 60 }
fn default_log_level() -> String { "info".to_string() }
fn default_log_format() -> String { "text".to_string() }
fn default_file_max_size() -> u64 { 104_857_600 } // 100MB
fn default_file_max_count() -> usize { 5 }
fn default_true() -> bool { true }

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            port: default_proxy_port(),
            auth_enabled: false,
            username: None,
            password: None,
        }
    }
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: default_compression_level(),
            adaptive: true,
            dictionary_path: None,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_memory_size: default_cache_size(),
            disk_path: None,
            default_ttl_html_secs: default_cache_ttl_html(),
            default_ttl_css_js_secs: default_cache_ttl_css_js(),
            default_ttl_images_secs: default_cache_ttl_images(),
        }
    }
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            concurrency: default_concurrency(),
            max_in_flight: default_max_in_flight(),
            batch_timeout_ms: default_batch_timeout_ms(),
            batch_threshold: default_batch_threshold(),
        }
    }
}

impl Default for DohConfigSection {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoints: default_doh_endpoints(),
            timeout_secs: default_doh_timeout_secs(),
            use_get_method: false,
        }
    }
}

impl Default for MtuConfig {
    fn default() -> Self {
        Self {
            min_size: default_min_mtu(),
            max_size: default_max_mtu(),
            probe_timeout_ms: default_probe_timeout_ms(),
            failure_threshold: default_failure_threshold(),
        }
    }
}

impl Default for LoadBalancerConfig {
    fn default() -> Self {
        Self {
            failure_timeout_secs: default_failure_timeout_secs(),
            window_size_secs: default_window_size_secs(),
            success_threshold: default_success_threshold(),
            weight_reduction: default_weight_reduction(),
        }
    }
}

impl Default for PriorityConfig {
    fn default() -> Self {
        Self {
            bandwidth_weights: default_bandwidth_weights(),
            starvation_timeout_secs: default_starvation_timeout_secs(),
        }
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_connections: default_max_connections(),
            max_streams_per_connection: default_max_streams(),
            idle_timeout_secs: default_idle_timeout_secs(),
        }
    }
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            initial_size: default_initial_buffer_size(),
            max_size: default_max_buffer_size(),
            buffer_capacity: default_buffer_capacity(),
        }
    }
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_queue_size: default_prefetch_queue_size(),
            resource_types: default_resource_types(),
        }
    }
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            blocklist_path: None,
            whitelist: Vec::new(),
            update_enabled: false,
            update_url: None,
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            http_port: default_metrics_port(),
            log_interval_secs: default_log_interval_secs(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            file_enabled: false,
            file_path: None,
            file_max_size: default_file_max_size(),
            file_max_count: default_file_max_count(),
        }
    }
}

impl Config {
    /// Load configuration from TOML file
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::VerySlipError::Config(format!("Failed to read config file: {}", e)))?;
        
        let config: Config = toml::from_str(&content)
            .map_err(|e| crate::VerySlipError::Config(format!("Failed to parse config: {}", e)))?;
        
        Ok(config)
    }
    
    /// Generate default configuration as TOML string
    pub fn generate_default() -> String {
        let config = Config {
            domains: vec!["s1.example.com".to_string()],
            resolvers: vec!["8.8.8.8:53".to_string()],
            proxy: ProxyConfig::default(),
            compression: CompressionConfig::default(),
            cache: CacheConfig::default(),
            query: QueryConfig::default(),
            doh: DohConfigSection::default(),
            mtu: MtuConfig::default(),
            load_balancer: LoadBalancerConfig::default(),
            priority: PriorityConfig::default(),
            connection: ConnectionConfig::default(),
            buffer: BufferConfig::default(),
            prefetch: PrefetchConfig::default(),
            filter: FilterConfig::default(),
            metrics: MetricsConfig::default(),
            logging: LoggingConfig::default(),
        };
        
        toml::to_string_pretty(&config).unwrap_or_else(|_| String::from("# Error generating config"))
    }
    
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        let mut errors = Vec::new();
        
        // Validate domains
        if self.domains.is_empty() {
            errors.push("At least one domain required".to_string());
        }
        if self.domains.len() > 50 {
            errors.push("Maximum 50 domains allowed".to_string());
        }
        
        // Validate resolvers
        if self.resolvers.is_empty() {
            errors.push("At least one resolver required".to_string());
        }
        
        // Validate compression level
        if self.compression.level < 1 || self.compression.level > 9 {
            errors.push("Compression level must be 1-9".to_string());
        }
        
        // Validate query concurrency
        if self.query.concurrency < 1 || self.query.concurrency > 32 {
            errors.push("Query concurrency must be 1-32".to_string());
        }
        
        // Validate MTU range
        if self.mtu.min_size < 512 || self.mtu.min_size > self.mtu.max_size {
            errors.push("Invalid MTU range".to_string());
        }
        
        // Validate bandwidth weights
        let sum: f32 = self.priority.bandwidth_weights.iter().sum();
        if (sum - 1.0).abs() > 0.01 {
            errors.push("Bandwidth weights must sum to 1.0".to_string());
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(crate::VerySlipError::Config(errors.join("; ")))
        }
    }
}

/// Get platform-specific config directory
pub fn config_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("veryslip-client")
    }
    
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("%APPDATA%"))
            .join("veryslip-client")
    }
    
    #[cfg(target_os = "macos")]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/Library/Application Support"))
            .join("veryslip-client")
    }
}

/// Get platform-specific cache directory
pub fn cache_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("veryslip-client")
    }
    
    #[cfg(target_os = "windows")]
    {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("%LOCALAPPDATA%"))
            .join("veryslip-client")
            .join("cache")
    }
    
    #[cfg(target_os = "macos")]
    {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/Library/Caches"))
            .join("veryslip-client")
    }
}
