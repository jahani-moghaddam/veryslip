use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use veryslip_client::compression::{CompressionConfig, CompressionEngine};

fn compression_benchmark(c: &mut Criterion) {
    let config = CompressionConfig {
        level: 3,
        dictionary_path: None,
        adaptive: false,
    };
    let engine = CompressionEngine::new(config).unwrap();
    
    // Test data: 1KB of HTML-like content
    let data_1kb = b"<html><head><title>Test</title></head><body><p>Hello World</p></body></html>".repeat(13);
    
    // Test data: 10KB of HTML-like content
    let data_10kb = data_1kb.repeat(10);
    
    // Test data: 100KB of HTML-like content
    let data_100kb = data_1kb.repeat(100);
    
    let mut group = c.benchmark_group("compression");
    
    group.throughput(Throughput::Bytes(data_1kb.len() as u64));
    group.bench_function("compress_1kb", |b| {
        b.iter(|| engine.compress(black_box(&data_1kb)))
    });
    
    group.throughput(Throughput::Bytes(data_10kb.len() as u64));
    group.bench_function("compress_10kb", |b| {
        b.iter(|| engine.compress(black_box(&data_10kb)))
    });
    
    group.throughput(Throughput::Bytes(data_100kb.len() as u64));
    group.bench_function("compress_100kb", |b| {
        b.iter(|| engine.compress(black_box(&data_100kb)))
    });
    
    // Decompress benchmarks
    let compressed_1kb = engine.compress(&data_1kb).unwrap();
    let compressed_10kb = engine.compress(&data_10kb).unwrap();
    let compressed_100kb = engine.compress(&data_100kb).unwrap();
    
    group.throughput(Throughput::Bytes(data_1kb.len() as u64));
    group.bench_function("decompress_1kb", |b| {
        b.iter(|| engine.decompress(black_box(&compressed_1kb)))
    });
    
    group.throughput(Throughput::Bytes(data_10kb.len() as u64));
    group.bench_function("decompress_10kb", |b| {
        b.iter(|| engine.decompress(black_box(&compressed_10kb)))
    });
    
    group.throughput(Throughput::Bytes(data_100kb.len() as u64));
    group.bench_function("decompress_100kb", |b| {
        b.iter(|| engine.decompress(black_box(&compressed_100kb)))
    });
    
    group.finish();
}

criterion_group!(benches, compression_benchmark);
criterion_main!(benches);
