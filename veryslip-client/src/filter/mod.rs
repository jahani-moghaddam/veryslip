use dashmap::{DashMap, DashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::path::Path;
use crate::Result;

/// Filter engine for blocking ads and trackers
pub struct FilterEngine {
    blocklist: Arc<DashMap<String, BlockReason>>,
    whitelist: Arc<DashSet<String>>,
    stats: Arc<FilterStats>,
}

/// Reason for blocking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    Advertisement,
    Tracker,
    Malware,
}

/// Filter statistics
#[derive(Debug, Default)]
pub struct FilterStats {
    pub requests_blocked: AtomicU64,
    pub bytes_saved: AtomicU64,
}

impl FilterEngine {
    /// Create new filter engine
    pub fn new() -> Self {
        Self {
            blocklist: Arc::new(DashMap::new()),
            whitelist: Arc::new(DashSet::new()),
            stats: Arc::new(FilterStats::default()),
        }
    }

    /// Load blocklist from file
    pub fn load_blocklist(&self, path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::VerySlipError::Config(format!("Failed to read blocklist: {}", e)))?;

        self.parse_blocklist(&content);
        Ok(())
    }

    /// Parse blocklist content
    fn parse_blocklist(&self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse domain and reason
            let (domain, reason) = if line.contains("tracker") {
                (line.split_whitespace().next().unwrap_or(line), BlockReason::Tracker)
            } else if line.contains("malware") {
                (line.split_whitespace().next().unwrap_or(line), BlockReason::Malware)
            } else {
                (line.split_whitespace().next().unwrap_or(line), BlockReason::Advertisement)
            };

            self.blocklist.insert(domain.to_lowercase(), reason);
        }
    }

    /// Load embedded default blocklist
    pub fn load_default_blocklist(&self) {
        // Embedded EasyList + EasyPrivacy subset
        const DEFAULT_BLOCKLIST: &str = include_str!("../../blocklist.txt");
        self.parse_blocklist(DEFAULT_BLOCKLIST);
    }

    /// Check if host should be blocked
    pub fn should_block(&self, host: &str) -> Option<BlockReason> {
        let host_lower = host.to_lowercase();

        // Check whitelist first
        if self.whitelist.contains(&host_lower) {
            return None;
        }

        // Exact match
        if let Some(reason) = self.blocklist.get(&host_lower) {
            self.stats.requests_blocked.fetch_add(1, Ordering::Relaxed);
            return Some(*reason);
        }

        // Suffix matching (check parent domains)
        let parts: Vec<&str> = host_lower.split('.').collect();
        for i in 1..parts.len() {
            let suffix = parts[i..].join(".");
            if let Some(reason) = self.blocklist.get(&suffix) {
                self.stats.requests_blocked.fetch_add(1, Ordering::Relaxed);
                return Some(*reason);
            }
        }

        // Wildcard matching
        for entry in self.blocklist.iter() {
            let pattern = entry.key();
            if pattern.starts_with("*.") {
                let domain_suffix = &pattern[2..];
                if host_lower.ends_with(domain_suffix) {
                    self.stats.requests_blocked.fetch_add(1, Ordering::Relaxed);
                    return Some(*entry.value());
                }
            }
        }

        None
    }

    /// Add domain to whitelist
    pub fn add_to_whitelist(&self, host: String) {
        self.whitelist.insert(host.to_lowercase());
    }

    /// Remove domain from whitelist
    pub fn remove_from_whitelist(&self, host: &str) {
        self.whitelist.remove(&host.to_lowercase());
    }

    /// Add domain to blocklist
    pub fn add_to_blocklist(&self, host: String, reason: BlockReason) {
        self.blocklist.insert(host.to_lowercase(), reason);
    }

    /// Remove domain from blocklist
    pub fn remove_from_blocklist(&self, host: &str) {
        self.blocklist.remove(&host.to_lowercase());
    }

    /// Record bytes saved from blocking
    pub fn record_bytes_saved(&self, bytes: u64) {
        self.stats.bytes_saved.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Get statistics
    pub fn stats(&self) -> FilterStatsSnapshot {
        FilterStatsSnapshot {
            requests_blocked: self.stats.requests_blocked.load(Ordering::Relaxed),
            bytes_saved: self.stats.bytes_saved.load(Ordering::Relaxed),
            blocklist_size: self.blocklist.len(),
            whitelist_size: self.whitelist.len(),
        }
    }

    /// Reload blocklist from file
    pub fn reload_blocklist(&self, path: &Path) -> Result<()> {
        self.blocklist.clear();
        self.load_blocklist(path)
    }
}

impl Default for FilterEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of filter statistics
#[derive(Debug, Clone)]
pub struct FilterStatsSnapshot {
    pub requests_blocked: u64,
    pub bytes_saved: u64,
    pub blocklist_size: usize,
    pub whitelist_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_filter_engine_creation() {
        let engine = FilterEngine::new();
        let stats = engine.stats();
        
        assert_eq!(stats.requests_blocked, 0);
        assert_eq!(stats.bytes_saved, 0);
    }

    #[test]
    fn test_exact_match_blocking() {
        let engine = FilterEngine::new();
        engine.add_to_blocklist("ads.example.com".to_string(), BlockReason::Advertisement);
        
        let result = engine.should_block("ads.example.com");
        assert_eq!(result, Some(BlockReason::Advertisement));
        
        let stats = engine.stats();
        assert_eq!(stats.requests_blocked, 1);
    }

    #[test]
    fn test_suffix_matching() {
        let engine = FilterEngine::new();
        engine.add_to_blocklist("doubleclick.net".to_string(), BlockReason::Advertisement);
        
        // Should block subdomain
        let result = engine.should_block("ad.doubleclick.net");
        assert_eq!(result, Some(BlockReason::Advertisement));
        
        // Should block deeper subdomain
        let result = engine.should_block("stats.ad.doubleclick.net");
        assert_eq!(result, Some(BlockReason::Advertisement));
    }

    #[test]
    fn test_wildcard_matching() {
        let engine = FilterEngine::new();
        engine.add_to_blocklist("*.tracker.com".to_string(), BlockReason::Tracker);
        
        let result = engine.should_block("analytics.tracker.com");
        assert_eq!(result, Some(BlockReason::Tracker));
    }

    #[test]
    fn test_whitelist() {
        let engine = FilterEngine::new();
        engine.add_to_blocklist("example.com".to_string(), BlockReason::Advertisement);
        engine.add_to_whitelist("example.com".to_string());
        
        // Should not block whitelisted domain
        let result = engine.should_block("example.com");
        assert_eq!(result, None);
    }

    #[test]
    fn test_case_insensitive() {
        let engine = FilterEngine::new();
        engine.add_to_blocklist("ADS.EXAMPLE.COM".to_string(), BlockReason::Advertisement);
        
        let result = engine.should_block("ads.example.com");
        assert_eq!(result, Some(BlockReason::Advertisement));
        
        let result = engine.should_block("ADS.EXAMPLE.COM");
        assert_eq!(result, Some(BlockReason::Advertisement));
    }

    #[test]
    fn test_parse_blocklist() {
        let engine = FilterEngine::new();
        let content = r#"
# Comment line
ads.example.com
tracker.example.com tracker
malware.example.com malware

# Another comment
analytics.example.com
"#;
        
        engine.parse_blocklist(content);
        
        assert_eq!(engine.should_block("ads.example.com"), Some(BlockReason::Advertisement));
        assert_eq!(engine.should_block("tracker.example.com"), Some(BlockReason::Tracker));
        assert_eq!(engine.should_block("malware.example.com"), Some(BlockReason::Malware));
        assert_eq!(engine.should_block("analytics.example.com"), Some(BlockReason::Advertisement));
    }

    #[test]
    fn test_load_blocklist_from_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "ads.example.com").unwrap();
        writeln!(file, "tracker.example.com tracker").unwrap();
        
        let engine = FilterEngine::new();
        engine.load_blocklist(file.path()).unwrap();
        
        assert_eq!(engine.should_block("ads.example.com"), Some(BlockReason::Advertisement));
        assert_eq!(engine.should_block("tracker.example.com"), Some(BlockReason::Tracker));
    }

    #[test]
    fn test_bytes_saved_tracking() {
        let engine = FilterEngine::new();
        
        engine.record_bytes_saved(1024);
        engine.record_bytes_saved(2048);
        
        let stats = engine.stats();
        assert_eq!(stats.bytes_saved, 3072);
    }

    #[test]
    fn test_blocklist_management() {
        let engine = FilterEngine::new();
        
        engine.add_to_blocklist("test.com".to_string(), BlockReason::Advertisement);
        assert!(engine.should_block("test.com").is_some());
        
        engine.remove_from_blocklist("test.com");
        assert!(engine.should_block("test.com").is_none());
    }

    #[test]
    fn test_whitelist_management() {
        let engine = FilterEngine::new();
        
        engine.add_to_blocklist("test.com".to_string(), BlockReason::Advertisement);
        engine.add_to_whitelist("test.com".to_string());
        assert!(engine.should_block("test.com").is_none());
        
        engine.remove_from_whitelist("test.com");
        assert!(engine.should_block("test.com").is_some());
    }
}
