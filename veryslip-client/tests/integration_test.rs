use veryslip_client::*;
use std::sync::Arc;

/// Integration test: End-to-end component creation
#[tokio::test]
async fn test_component_initialization() {
    // Create all components to verify they can be initialized
    let filter = Arc::new(filter::FilterEngine::new());
    
    let cache_config = cache::CacheConfig {
        max_memory_size: 10 * 1024 * 1024,
        disk_path: std::env::temp_dir().join("veryslip_test_cache"),
        default_ttl: std::time::Duration::from_secs(3600),
    };
    let cache = Arc::new(cache::CacheManager::new(cache_config).unwrap());
    
    let priority_config = priority::PriorityConfig::default();
    let priority_queue = Arc::new(priority::PriorityQueue::new(priority_config));
    
    let compression_config = compression::CompressionConfig {
        level: 3,
        dictionary_path: None,
        adaptive: true,
    };
    let compression = Arc::new(compression::CompressionEngine::new(compression_config).unwrap());
    
    let domains = vec!["test.example.com".to_string()];
    let lb_config = load_balancer::LoadBalancerConfig::default();
    let load_balancer = Arc::new(load_balancer::LoadBalancer::new(domains.clone(), lb_config));
    
    let buffer_config = buffer::BufferPoolConfig::default();
    let buffer_pool = Arc::new(buffer::BufferPool::new(buffer_config));
    
    let query_config = query::QueryConfig::default();
    let query_engine = Arc::new(query::QueryEngine::new(
        query_config,
        load_balancer.clone(),
        buffer_pool,
    ));
    
    let connection_config = connection::ConnectionPoolConfig::default();
    let connection_pool = Arc::new(
        connection::ConnectionPool::new(connection_config, domains[0].clone()).unwrap()
    );
    
    let prefetch_config = prefetch::PrefetchConfig::default();
    let prefetch = Arc::new(prefetch::PrefetchEngine::new(
        prefetch_config,
        cache.clone(),
        priority_queue.clone(),
    ));
    
    let _pipeline = Arc::new(pipeline::Pipeline::new(
        filter,
        cache,
        priority_queue,
        compression,
        query_engine,
        load_balancer,
        connection_pool,
        prefetch,
    ));
    
    // Test passes if all components initialize successfully
    assert!(true);
}

/// Test compression roundtrip
#[test]
fn test_compression_roundtrip() {
    let config = compression::CompressionConfig {
        level: 5,
        dictionary_path: None,
        adaptive: false,
    };
    let engine = compression::CompressionEngine::new(config).unwrap();
    
    let data = b"Hello, World! This is a test message for compression.".repeat(10);
    let compressed = engine.compress(&data, Some("text/html")).unwrap();
    let decompressed = engine.decompress(&compressed).unwrap();
    
    assert_eq!(data.to_vec(), decompressed);
    assert!(compressed.len() < data.len());
}

/// Test DNS encoding/decoding
#[test]
fn test_dns_encoding() {
    let data = b"Test data for DNS tunnel encoding";
    let encoded = dns::base32::encode_base32(data);
    let decoded = dns::base32::decode_base32(&encoded).unwrap();
    
    assert_eq!(data.to_vec(), decoded);
}

/// Test load balancing
#[test]
fn test_load_balancing() {
    let domains = vec![
        "domain1.example.com".to_string(),
        "domain2.example.com".to_string(),
        "domain3.example.com".to_string(),
    ];
    let config = load_balancer::LoadBalancerConfig::default();
    let lb = load_balancer::LoadBalancer::new(domains.clone(), config);
    
    // Select domains multiple times
    let mut domain_names = Vec::new();
    for _ in 0..30 {
        let domain_state = lb.select_domain().unwrap();
        domain_names.push(domain_state.domain.clone());
    }
    
    // Should use all domains
    let unique: std::collections::HashSet<_> = domain_names.iter().collect();
    assert!(unique.len() >= 2); // At least 2 domains should be used
}

/// Test filter engine
#[test]
fn test_filter_engine() {
    let filter = filter::FilterEngine::new();
    
    // Add some blocked domains
    filter.add_to_blocklist("ads.example.com".to_string(), filter::BlockReason::Advertisement);
    filter.add_to_blocklist("tracker.example.com".to_string(), filter::BlockReason::Tracker);
    
    assert!(filter.should_block("ads.example.com").is_some());
    assert!(filter.should_block("tracker.example.com").is_some());
    assert!(filter.should_block("legitimate.example.com").is_none());
}
