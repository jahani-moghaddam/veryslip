use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use veryslip_client::dns::base32::{encode_base32, decode_base32};

fn base32_benchmark(c: &mut Criterion) {
    // Test data of various sizes
    let data_100b = vec![0u8; 100];
    let data_1kb = vec![0u8; 1024];
    let data_10kb = vec![0u8; 10 * 1024];
    
    let mut group = c.benchmark_group("base32");
    
    // Encoding benchmarks
    group.throughput(Throughput::Bytes(data_100b.len() as u64));
    group.bench_function("encode_100b", |b| {
        b.iter(|| encode_base32(black_box(&data_100b)))
    });
    
    group.throughput(Throughput::Bytes(data_1kb.len() as u64));
    group.bench_function("encode_1kb", |b| {
        b.iter(|| encode_base32(black_box(&data_1kb)))
    });
    
    group.throughput(Throughput::Bytes(data_10kb.len() as u64));
    group.bench_function("encode_10kb", |b| {
        b.iter(|| encode_base32(black_box(&data_10kb)))
    });
    
    // Decoding benchmarks
    let encoded_100b = encode_base32(&data_100b);
    let encoded_1kb = encode_base32(&data_1kb);
    let encoded_10kb = encode_base32(&data_10kb);
    
    group.throughput(Throughput::Bytes(data_100b.len() as u64));
    group.bench_function("decode_100b", |b| {
        b.iter(|| decode_base32(black_box(&encoded_100b)))
    });
    
    group.throughput(Throughput::Bytes(data_1kb.len() as u64));
    group.bench_function("decode_1kb", |b| {
        b.iter(|| decode_base32(black_box(&encoded_1kb)))
    });
    
    group.throughput(Throughput::Bytes(data_10kb.len() as u64));
    group.bench_function("decode_10kb", |b| {
        b.iter(|| decode_base32(black_box(&encoded_10kb)))
    });
    
    group.finish();
}

criterion_group!(benches, base32_benchmark);
criterion_main!(benches);
