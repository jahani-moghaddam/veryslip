use criterion::{black_box, criterion_group, criterion_main, Criterion};
use veryslip_client::cache::{CacheConfig, CacheManager, CacheKey, CacheEntry};
use std::time::Duration;

fn cache_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let config = CacheConfig {
        max_memory_size: 100 * 1024 * 1024,
        disk_path: std::env::temp_dir().join("veryslip_bench_cache"),
        default_ttl: Duration::from_secs(3600),
    };
    let cache = CacheManager::new(config).unwrap();
    
    // Prepare test data
    let key = CacheKey {
        url: "http://example.com/test".to_string(),
        vary_headers: vec![],
    };
    
    let entry = CacheEntry {
        url: "http://example.com/test".to_string(),
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/html".to_string())],
        body: vec![0u8; 10 * 1024], // 10KB
        stored_at: std::time::Instant::now(),
        expires_at: Some(std::time::Instant::now() + Duration::from_secs(3600)),
        etag: None,
        last_modified: None,
    };
    
    // Pre-populate cache
    rt.block_on(async {
        cache.put(key.clone(), entry.clone()).await.unwrap();
    });
    
    let mut group = c.benchmark_group("cache");
    
    group.bench_function("cache_get", |b| {
        b.to_async(&rt).iter(|| async {
            cache.get(black_box(&key)).await
        })
    });
    
    group.bench_function("cache_put", |b| {
        b.to_async(&rt).iter(|| async {
            cache.put(black_box(key.clone()), black_box(entry.clone())).await
        })
    });
    
    group.finish();
}

criterion_group!(benches, cache_benchmark);
criterion_main!(benches);
