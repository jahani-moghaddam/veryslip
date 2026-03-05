use crate::{Result, VerySlipError};
use crate::config::Config;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration reload manager
pub struct ConfigReloader {
    config_path: PathBuf,
    current_config: Arc<RwLock<Config>>,
}

impl ConfigReloader {
    /// Create new config reloader
    pub fn new(config_path: PathBuf, initial_config: Config) -> Self {
        Self {
            config_path,
            current_config: Arc::new(RwLock::new(initial_config)),
        }
    }

    /// Get current configuration
    pub async fn get_config(&self) -> Config {
        self.current_config.read().await.clone()
    }

    /// Reload configuration from file
    pub async fn reload(&self) -> Result<ReloadResult> {
        tracing::info!("Reloading configuration from {:?}", self.config_path);

        // Load new config
        let new_config = Config::load(&self.config_path)?;

        // Validate new config
        new_config.validate()?;

        // Get current config for comparison
        let old_config = self.current_config.read().await.clone();

        // Determine what changed
        let changes = detect_changes(&old_config, &new_config);

        // Update config
        *self.current_config.write().await = new_config;

        tracing::info!("Configuration reloaded successfully");
        
        Ok(ReloadResult {
            changes: changes.clone(),
            requires_restart: requires_restart(&changes),
        })
    }

    /// Start file watcher (for platforms that support it)
    #[cfg(not(target_os = "windows"))]
    pub async fn start_signal_handler(self: Arc<Self>) -> Result<()> {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sighup = signal(SignalKind::hangup())
            .map_err(|e| VerySlipError::InvalidState(format!("Failed to register SIGHUP handler: {}", e)))?;

        tracing::info!("SIGHUP handler registered for config reload");

        loop {
            sighup.recv().await;
            tracing::info!("Received SIGHUP signal");

            match self.reload().await {
                Ok(result) => {
                    tracing::info!("Config reloaded: {:?}", result.changes);
                    if result.requires_restart {
                        tracing::warn!("Some changes require restart: {:?}", result.changes);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to reload config: {}", e);
                }
            }
        }
    }

    /// Start file watcher for Windows
    #[cfg(target_os = "windows")]
    pub async fn start_file_watcher(self: Arc<Self>) -> Result<()> {
        use notify::{Watcher, RecursiveMode, Event};
        use tokio::sync::mpsc;

        let (tx, mut rx) = mpsc::channel(10);

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        }).map_err(|e| VerySlipError::InvalidState(format!("Failed to create file watcher: {}", e)))?;

        watcher.watch(&self.config_path, RecursiveMode::NonRecursive)
            .map_err(|e| VerySlipError::InvalidState(format!("Failed to watch config file: {}", e)))?;

        tracing::info!("File watcher started for config reload");

        loop {
            if let Some(event) = rx.recv().await {
                if event.kind.is_modify() {
                    tracing::info!("Config file modified, reloading...");

                    // Add small delay to ensure file write is complete
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    match self.reload().await {
                        Ok(result) => {
                            tracing::info!("Config reloaded: {:?}", result.changes);
                            if result.requires_restart {
                                tracing::warn!("Some changes require restart: {:?}", result.changes);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to reload config: {}", e);
                        }
                    }
                }
            }
        }
    }
}

/// Result of configuration reload
#[derive(Debug, Clone)]
pub struct ReloadResult {
    pub changes: Vec<ConfigChange>,
    pub requires_restart: bool,
}

/// Type of configuration change
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigChange {
    Domains,
    Resolvers,
    ProxyPort,
    LogLevel,
    Blocklist,
    CompressionLevel,
    CacheSize,
    QueryConcurrency,
    MtuSettings,
    Other(String),
}

/// Detect changes between old and new config
fn detect_changes(old: &Config, new: &Config) -> Vec<ConfigChange> {
    let mut changes = Vec::new();

    if old.domains != new.domains {
        changes.push(ConfigChange::Domains);
    }

    if old.resolvers != new.resolvers {
        changes.push(ConfigChange::Resolvers);
    }

    if old.proxy.port != new.proxy.port {
        changes.push(ConfigChange::ProxyPort);
    }

    if old.logging.level != new.logging.level {
        changes.push(ConfigChange::LogLevel);
    }

    if old.filter.blocklist_path != new.filter.blocklist_path {
        changes.push(ConfigChange::Blocklist);
    }

    if old.compression.level != new.compression.level {
        changes.push(ConfigChange::CompressionLevel);
    }

    if old.cache.max_memory_size != new.cache.max_memory_size {
        changes.push(ConfigChange::CacheSize);
    }

    if old.query.concurrency != new.query.concurrency {
        changes.push(ConfigChange::QueryConcurrency);
    }

    if old.mtu.min_size != new.mtu.min_size || old.mtu.max_size != new.mtu.max_size {
        changes.push(ConfigChange::MtuSettings);
    }

    changes
}

/// Check if changes require restart
fn requires_restart(changes: &[ConfigChange]) -> bool {
    changes.iter().any(|change| matches!(
        change,
        ConfigChange::ProxyPort | ConfigChange::Resolvers
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn create_test_config() -> Config {
        Config {
            domains: vec!["test.example.com".to_string()],
            resolvers: vec!["8.8.8.8:53".to_string()],
            proxy: Default::default(),
            compression: Default::default(),
            cache: Default::default(),
            query: Default::default(),
            doh: Default::default(),
            mtu: Default::default(),
            load_balancer: Default::default(),
            priority: Default::default(),
            connection: Default::default(),
            buffer: Default::default(),
            prefetch: Default::default(),
            filter: Default::default(),
            metrics: Default::default(),
            logging: Default::default(),
        }
    }

    #[test]
    fn test_detect_changes_none() {
        let config1 = create_test_config();
        let config2 = create_test_config();

        let changes = detect_changes(&config1, &config2);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_detect_changes_domains() {
        let mut config1 = create_test_config();
        let mut config2 = create_test_config();

        config1.domains = vec!["example1.com".to_string()];
        config2.domains = vec!["example2.com".to_string()];

        let changes = detect_changes(&config1, &config2);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0], ConfigChange::Domains);
    }

    #[test]
    fn test_detect_changes_log_level() {
        let mut config1 = create_test_config();
        let mut config2 = create_test_config();

        config1.logging.level = "info".to_string();
        config2.logging.level = "debug".to_string();

        let changes = detect_changes(&config1, &config2);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0], ConfigChange::LogLevel);
    }

    #[test]
    fn test_requires_restart() {
        assert!(requires_restart(&[ConfigChange::ProxyPort]));
        assert!(requires_restart(&[ConfigChange::Resolvers]));
        assert!(!requires_restart(&[ConfigChange::LogLevel]));
        assert!(!requires_restart(&[ConfigChange::Domains]));
    }

    #[test]
    fn test_requires_restart_mixed() {
        let changes = vec![
            ConfigChange::LogLevel,
            ConfigChange::ProxyPort,
            ConfigChange::Domains,
        ];
        assert!(requires_restart(&changes));
    }
}
