use crate::{Result, VerySlipError};
use crate::cache::CacheManager;
use crate::priority::PriorityQueue;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{RcDom, NodeData, Handle};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use url::Url;

/// Prefetch engine for resource extraction
pub struct PrefetchEngine {
    config: PrefetchConfig,
    cache: Arc<CacheManager>,
    priority_queue: Arc<PriorityQueue>,
    stats: PrefetchStats,
}

/// Prefetch configuration
#[derive(Debug, Clone)]
pub struct PrefetchConfig {
    pub enabled: bool,
    pub max_queue_size: usize,
    pub resource_types: Vec<ResourceType>,
    pub max_resource_size: usize,
    pub allow_cross_origin: bool,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_queue_size: 50,
            resource_types: vec![
                ResourceType::Stylesheet,
                ResourceType::Script,
                ResourceType::Image,
                ResourceType::Font,
            ],
            max_resource_size: 5 * 1024 * 1024, // 5MB
            allow_cross_origin: false,
        }
    }
}

/// Resource types to prefetch
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Stylesheet,
    Script,
    Image,
    Font,
}

/// Prefetch statistics
#[derive(Debug, Default)]
pub struct PrefetchStats {
    pub resources_extracted: AtomicU64,
    pub resources_queued: AtomicU64,
    pub resources_skipped: AtomicU64,
}

impl PrefetchEngine {
    /// Create new prefetch engine
    pub fn new(
        config: PrefetchConfig,
        cache: Arc<CacheManager>,
        priority_queue: Arc<PriorityQueue>,
    ) -> Self {
        Self {
            config,
            cache,
            priority_queue,
            stats: PrefetchStats::default(),
        }
    }

    /// Process HTML and extract resources
    pub async fn process_html(&self, url: &str, html: &str) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let base_url = Url::parse(url)
            .map_err(|e| VerySlipError::Parse(format!("Invalid URL: {}", e)))?;

        let resources = self.extract_resources(html, &base_url)?;
        
        for resource_url in resources {
            self.stats.resources_extracted.fetch_add(1, Ordering::Relaxed);

            // Skip if already in cache
            let cache_key = crate::cache::CacheKey::new(resource_url.clone());
            if self.cache.get(&cache_key).is_some() {
                self.stats.resources_skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            // Check queue size limit
            let queue_sizes = self.priority_queue.queue_lengths();
            let total_queued: usize = queue_sizes.iter().sum();
            if total_queued >= self.config.max_queue_size {
                self.stats.resources_skipped.fetch_add(1, Ordering::Relaxed);
                break;
            }

            // Queue prefetch request at low priority
            let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
            let prefetch_request = crate::priority::PendingRequest {
                url: resource_url.clone(),
                method: "GET".to_string(),
                headers: vec![
                    ("Purpose".to_string(), "prefetch".to_string()),
                    ("User-Agent".to_string(), "VerySlip-Client/0.1.0".to_string()),
                ],
                body: vec![],
                priority: crate::priority::Priority::Low,
                enqueued_at: std::time::Instant::now(),
                response_tx,
            };

            // Enqueue the prefetch request
            if let Err(e) = self.priority_queue.enqueue(prefetch_request).await {
                tracing::debug!("Failed to enqueue prefetch for {}: {}", resource_url, e);
                self.stats.resources_skipped.fetch_add(1, Ordering::Relaxed);
            } else {
                self.stats.resources_queued.fetch_add(1, Ordering::Relaxed);
            }
        }

        Ok(())
    }

    /// Extract resource URLs from HTML
    pub fn extract_resources(&self, html: &str, base_url: &Url) -> Result<Vec<String>> {
        let dom = parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .map_err(|e| VerySlipError::Parse(format!("HTML parse error: {}", e)))?;

        let mut resources = Vec::new();
        self.walk_dom(&dom.document, base_url, &mut resources);

        // Deduplicate
        resources.sort();
        resources.dedup();

        Ok(resources)
    }

    /// Walk DOM tree and extract resource URLs
    fn walk_dom(&self, handle: &Handle, base_url: &Url, resources: &mut Vec<String>) {
        let node = handle;
        
        if let NodeData::Element { ref name, ref attrs, .. } = node.data {
            let tag_name = name.local.as_ref();
            let attrs = attrs.borrow();

            match tag_name {
                "link" => {
                    // <link rel="stylesheet" href="...">
                    // <link rel="preload" href="...">
                    let rel = attrs.iter()
                        .find(|attr| attr.name.local.as_ref() == "rel")
                        .map(|attr| attr.value.as_ref());
                    
                    let href = attrs.iter()
                        .find(|attr| attr.name.local.as_ref() == "href")
                        .map(|attr| attr.value.as_ref());

                    if let (Some(rel), Some(href)) = (rel, href) {
                        if (rel == "stylesheet" && self.config.resource_types.contains(&ResourceType::Stylesheet))
                            || rel == "preload" {
                            if let Some(url) = self.resolve_url(base_url, href) {
                                resources.push(url);
                            }
                        }
                    }
                }
                "script" => {
                    // <script src="...">
                    if self.config.resource_types.contains(&ResourceType::Script) {
                        if let Some(src) = attrs.iter()
                            .find(|attr| attr.name.local.as_ref() == "src")
                            .map(|attr| attr.value.as_ref()) {
                            if let Some(url) = self.resolve_url(base_url, src) {
                                resources.push(url);
                            }
                        }
                    }
                }
                "img" => {
                    // <img src="...">
                    if self.config.resource_types.contains(&ResourceType::Image) {
                        if let Some(src) = attrs.iter()
                            .find(|attr| attr.name.local.as_ref() == "src")
                            .map(|attr| attr.value.as_ref()) {
                            if let Some(url) = self.resolve_url(base_url, src) {
                                resources.push(url);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Recursively walk children
        for child in node.children.borrow().iter() {
            self.walk_dom(child, base_url, resources);
        }
    }

    /// Resolve relative URL against base URL
    fn resolve_url(&self, base_url: &Url, href: &str) -> Option<String> {
        // Skip data: URLs
        if href.starts_with("data:") {
            return None;
        }

        // Skip javascript: URLs
        if href.starts_with("javascript:") {
            return None;
        }

        // Parse and resolve URL
        match base_url.join(href) {
            Ok(url) => {
                // Check cross-origin
                if !self.config.allow_cross_origin {
                    if url.origin() != base_url.origin() {
                        return None;
                    }
                }
                Some(url.to_string())
            }
            Err(_) => None,
        }
    }

    /// Get statistics
    pub fn stats(&self) -> PrefetchStatsSnapshot {
        PrefetchStatsSnapshot {
            resources_extracted: self.stats.resources_extracted.load(Ordering::Relaxed),
            resources_queued: self.stats.resources_queued.load(Ordering::Relaxed),
            resources_skipped: self.stats.resources_skipped.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of prefetch statistics
#[derive(Debug, Clone)]
pub struct PrefetchStatsSnapshot {
    pub resources_extracted: u64,
    pub resources_queued: u64,
    pub resources_skipped: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{CacheManager, CacheConfig};
    use crate::priority::PriorityConfig;

    fn create_test_engine() -> PrefetchEngine {
        let cache_config = CacheConfig::default();
        let cache = Arc::new(CacheManager::new(cache_config).unwrap());
        
        let priority_config = PriorityConfig::default();
        let priority_queue = Arc::new(PriorityQueue::new(priority_config));
        
        let prefetch_config = PrefetchConfig::default();
        PrefetchEngine::new(prefetch_config, cache, priority_queue)
    }

    #[test]
    fn test_prefetch_engine_creation() {
        let engine = create_test_engine();
        let stats = engine.stats();
        
        assert_eq!(stats.resources_extracted, 0);
        assert_eq!(stats.resources_queued, 0);
    }

    #[test]
    fn test_extract_stylesheet() {
        let engine = create_test_engine();
        let html = r#"
            <html>
            <head>
                <link rel="stylesheet" href="/style.css">
            </head>
            </html>
        "#;
        
        let base_url = Url::parse("https://example.com").unwrap();
        let resources = engine.extract_resources(html, &base_url).unwrap();
        
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0], "https://example.com/style.css");
    }

    #[test]
    fn test_extract_script() {
        let engine = create_test_engine();
        let html = r#"
            <html>
            <body>
                <script src="/app.js"></script>
            </body>
            </html>
        "#;
        
        let base_url = Url::parse("https://example.com").unwrap();
        let resources = engine.extract_resources(html, &base_url).unwrap();
        
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0], "https://example.com/app.js");
    }

    #[test]
    fn test_extract_image() {
        let engine = create_test_engine();
        let html = r#"
            <html>
            <body>
                <img src="/logo.png">
            </body>
            </html>
        "#;
        
        let base_url = Url::parse("https://example.com").unwrap();
        let resources = engine.extract_resources(html, &base_url).unwrap();
        
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0], "https://example.com/logo.png");
    }

    #[test]
    fn test_extract_multiple_resources() {
        let engine = create_test_engine();
        let html = r#"
            <html>
            <head>
                <link rel="stylesheet" href="/style.css">
                <script src="/app.js"></script>
            </head>
            <body>
                <img src="/logo.png">
                <img src="/banner.jpg">
            </body>
            </html>
        "#;
        
        let base_url = Url::parse("https://example.com").unwrap();
        let resources = engine.extract_resources(html, &base_url).unwrap();
        
        assert_eq!(resources.len(), 4);
    }

    #[test]
    fn test_resolve_relative_url() {
        let engine = create_test_engine();
        let base_url = Url::parse("https://example.com/page/index.html").unwrap();
        
        let url = engine.resolve_url(&base_url, "../style.css");
        assert_eq!(url, Some("https://example.com/style.css".to_string()));
        
        let url = engine.resolve_url(&base_url, "/absolute.css");
        assert_eq!(url, Some("https://example.com/absolute.css".to_string()));
    }

    #[test]
    fn test_skip_data_urls() {
        let engine = create_test_engine();
        let base_url = Url::parse("https://example.com").unwrap();
        
        let url = engine.resolve_url(&base_url, "data:image/png;base64,iVBORw0KG");
        assert_eq!(url, None);
    }

    #[test]
    fn test_skip_javascript_urls() {
        let engine = create_test_engine();
        let base_url = Url::parse("https://example.com").unwrap();
        
        let url = engine.resolve_url(&base_url, "javascript:void(0)");
        assert_eq!(url, None);
    }

    #[test]
    fn test_cross_origin_filtering() {
        let cache_config = CacheConfig::default();
        let cache = Arc::new(CacheManager::new(cache_config).unwrap());
        
        let priority_config = PriorityConfig::default();
        let priority_queue = Arc::new(PriorityQueue::new(priority_config));
        
        let mut prefetch_config = PrefetchConfig::default();
        prefetch_config.allow_cross_origin = false;
        
        let engine = PrefetchEngine::new(prefetch_config, cache, priority_queue);
        let base_url = Url::parse("https://example.com").unwrap();
        
        // Same origin - should be allowed
        let url = engine.resolve_url(&base_url, "/style.css");
        assert!(url.is_some());
        
        // Cross origin - should be blocked
        let url = engine.resolve_url(&base_url, "https://other.com/style.css");
        assert_eq!(url, None);
    }

    #[test]
    fn test_deduplication() {
        let engine = create_test_engine();
        let html = r#"
            <html>
            <body>
                <img src="/logo.png">
                <img src="/logo.png">
                <img src="/logo.png">
            </body>
            </html>
        "#;
        
        let base_url = Url::parse("https://example.com").unwrap();
        let resources = engine.extract_resources(html, &base_url).unwrap();
        
        // Should deduplicate
        assert_eq!(resources.len(), 1);
    }

    #[test]
    fn test_preload_links() {
        let engine = create_test_engine();
        let html = r#"
            <html>
            <head>
                <link rel="preload" href="/font.woff2" as="font">
            </head>
            </html>
        "#;
        
        let base_url = Url::parse("https://example.com").unwrap();
        let resources = engine.extract_resources(html, &base_url).unwrap();
        
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0], "https://example.com/font.woff2");
    }
}
