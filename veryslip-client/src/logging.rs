use crate::{Result, VerySlipError};
use std::path::PathBuf;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Logging configuration
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub level: String,
    pub format: LogFormat,
    pub output: LogOutput,
    pub file_path: Option<PathBuf>,
    pub rotation_size: u64,
    pub max_files: usize,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Text,
            output: LogOutput::Stdout,
            file_path: None,
            rotation_size: 100 * 1024 * 1024, // 100MB
            max_files: 5,
        }
    }
}

/// Log format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

/// Log output destination
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogOutput {
    Stdout,
    File,
    Both,
}

/// Initialize logging system
pub fn init_logging(config: &LogConfig) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.level))
        .map_err(|e| VerySlipError::InvalidState(format!("Invalid log level: {}", e)))?;

    match config.output {
        LogOutput::Stdout => {
            init_stdout_logging(config, env_filter)?;
        }
        LogOutput::File => {
            init_file_logging(config, env_filter)?;
        }
        LogOutput::Both => {
            init_both_logging(config, env_filter)?;
        }
    }

    Ok(())
}

fn init_stdout_logging(config: &LogConfig, env_filter: EnvFilter) -> Result<()> {
    match config.format {
        LogFormat::Text => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .with_target(true)
                        .with_thread_ids(false)
                        .with_thread_names(false)
                        .with_ansi(true)
                )
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .json()
                        .with_target(true)
                        .with_current_span(true)
                )
                .init();
        }
    }
    Ok(())
}

fn init_file_logging(config: &LogConfig, env_filter: EnvFilter) -> Result<()> {
    let file_path = config.file_path.as_ref()
        .ok_or_else(|| VerySlipError::InvalidState("File path required for file logging".to_string()))?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::NEVER)
        .filename_prefix(file_path.file_name().unwrap().to_str().unwrap())
        .max_log_files(config.max_files)
        .build(file_path.parent().unwrap())
        .map_err(|e| VerySlipError::InvalidState(format!("Failed to create file appender: {}", e)))?;

    match config.format {
        LogFormat::Text => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .with_writer(file_appender)
                        .with_target(true)
                        .with_ansi(false)
                )
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(file_appender)
                        .with_target(true)
                        .with_current_span(true)
                )
                .init();
        }
    }
    Ok(())
}

fn init_both_logging(config: &LogConfig, env_filter: EnvFilter) -> Result<()> {
    let file_path = config.file_path.as_ref()
        .ok_or_else(|| VerySlipError::InvalidState("File path required for file logging".to_string()))?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::NEVER)
        .filename_prefix(file_path.file_name().unwrap().to_str().unwrap())
        .max_log_files(config.max_files)
        .build(file_path.parent().unwrap())
        .map_err(|e| VerySlipError::InvalidState(format!("Failed to create file appender: {}", e)))?;

    match config.format {
        LogFormat::Text => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .with_target(true)
                        .with_ansi(true)
                        .with_filter(tracing_subscriber::filter::LevelFilter::INFO)
                )
                .with(
                    fmt::layer()
                        .with_writer(file_appender)
                        .with_target(true)
                        .with_ansi(false)
                )
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .with_target(true)
                        .with_ansi(true)
                        .with_filter(tracing_subscriber::filter::LevelFilter::INFO)
                )
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(file_appender)
                        .with_target(true)
                        .with_current_span(true)
                )
                .init();
        }
    }
    Ok(())
}

/// Log rotation helper
pub struct LogRotation {
    file_path: PathBuf,
    max_size: u64,
    max_files: usize,
}

impl LogRotation {
    pub fn new(file_path: PathBuf, max_size: u64, max_files: usize) -> Self {
        Self {
            file_path,
            max_size,
            max_files,
        }
    }

    /// Check if rotation is needed and perform it
    pub fn check_and_rotate(&self) -> Result<()> {
        if !self.file_path.exists() {
            return Ok(());
        }

        let metadata = std::fs::metadata(&self.file_path)?;

        if metadata.len() >= self.max_size {
            self.rotate()?;
        }

        Ok(())
    }

    fn rotate(&self) -> Result<()> {
        // Rotate existing files
        for i in (1..self.max_files).rev() {
            let old_path = self.rotated_path(i);
            let new_path = self.rotated_path(i + 1);

            if old_path.exists() {
                if i + 1 > self.max_files {
                    // Delete oldest file
                    std::fs::remove_file(&old_path)?;
                } else {
                    std::fs::rename(&old_path, &new_path)?;
                }
            }
        }

        // Rotate current file to .1
        if self.file_path.exists() {
            std::fs::rename(&self.file_path, self.rotated_path(1))?;
        }

        Ok(())
    }

    fn rotated_path(&self, index: usize) -> PathBuf {
        let mut path = self.file_path.clone();
        let file_name = path.file_name().unwrap().to_str().unwrap();
        path.set_file_name(format!("{}.{}", file_name, index));
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_log_config_default() {
        let config = LogConfig::default();
        assert_eq!(config.level, "info");
        assert_eq!(config.format, LogFormat::Text);
        assert_eq!(config.output, LogOutput::Stdout);
        assert_eq!(config.rotation_size, 100 * 1024 * 1024);
        assert_eq!(config.max_files, 5);
    }

    #[test]
    fn test_log_rotation_path() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");
        
        let rotation = LogRotation::new(log_path.clone(), 1024, 5);
        
        assert_eq!(rotation.rotated_path(1), temp_dir.path().join("test.log.1"));
        assert_eq!(rotation.rotated_path(2), temp_dir.path().join("test.log.2"));
    }

    #[test]
    fn test_log_rotation() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");
        
        // Create test log file
        std::fs::write(&log_path, "test content").unwrap();
        
        let rotation = LogRotation::new(log_path.clone(), 5, 3);
        rotation.rotate().unwrap();
        
        // Original should be moved to .1
        assert!(!log_path.exists());
        assert!(temp_dir.path().join("test.log.1").exists());
    }

    #[test]
    fn test_log_rotation_multiple() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");
        
        let rotation = LogRotation::new(log_path.clone(), 5, 3);
        
        // Create and rotate multiple times
        for i in 1..=5 {
            std::fs::write(&log_path, format!("content {}", i)).unwrap();
            rotation.rotate().unwrap();
        }
        
        // Should have .1, .2, .3 (max 3 files)
        assert!(temp_dir.path().join("test.log.1").exists());
        assert!(temp_dir.path().join("test.log.2").exists());
        assert!(temp_dir.path().join("test.log.3").exists());
        assert!(!temp_dir.path().join("test.log.4").exists());
    }
}
