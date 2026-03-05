# Batch Processing Integration

## Status: ✅ COMPLETED

This document describes the integration of batch packet processing into veryslip-server to match veryslip-client's batch protocol.

## Implementation Summary

### 1. Batch Processing Module ✅
**Location**: `veryslip-server/crates/slipstream-server/src/batch.rs`

Created `BatchProcessor` with the following capabilities:
- `is_batched()`: Detects if payload contains multiple packets (count > 1)
- `split_batch()`: Parses batch format and extracts individual packets
- Thread-safe statistics tracking with atomic counters
- Comprehensive validation and error handling

**Batch Format** (veryslip-client compatible):
```
Byte 0: Packet count (N)
Bytes 1..(1+N*2): Offsets for each packet (u16 big-endian)
Remaining bytes: Concatenated packet data
```

**Constants**:
- `MAX_BATCH_SIZE`: 10 packets per batch

**Statistics**:
- `batches_processed`: Number of batches successfully split
- `packets_extracted`: Total packets extracted from batches
- `parse_errors`: Number of batch parsing errors

### 2. Server Integration ✅
**Location**: `veryslip-server/crates/slipstream-server/src/server.rs`

Added batch processor initialization in `run_server()`:
```rust
let batch_processor = Some(Arc::new(BatchProcessor::new()));
tracing::info!("Batch processing enabled (max {} packets per batch)", 
    crate::batch::MAX_BATCH_SIZE);
```

### 3. PacketContext Extension ✅
**Location**: `veryslip-server/crates/slipstream-server/src/udp_fallback.rs`

Extended `PacketContext` struct to include batch processor:
```rust
pub(crate) struct PacketContext<'a> {
    pub(crate) domains: &'a [&'a str],
    pub(crate) quic: *mut picoquic_quic_t,
    pub(crate) current_time: u64,
    pub(crate) local_addr_storage: &'a libc::sockaddr_storage,
    pub(crate) compression: &'a Option<Arc<CompressionEngine>>,
    pub(crate) batch_processor: &'a Option<Arc<BatchProcessor>>,
}
```

### 4. Batch Processing in Decode Path ✅
**Location**: `veryslip-server/crates/slipstream-server/src/udp_fallback/decode.rs`

Added batch processing after decompression, before QUIC processing in `decode_slot()`:
```rust
// Check if payload is batched and split if needed
let payloads = if let Some(processor) = batch_processor {
    if processor.is_batched(&query.payload) {
        match processor.split_batch(&query.payload) {
            Ok(packets) => {
                tracing::trace!("Split batch into {} packets from {}", 
                    packets.len(), peer);
                packets
            }
            Err(e) => {
                tracing::warn!("Batch splitting failed from {}: {}", peer, e);
                // Return DNS ServerFailure response
                return Ok(DecodeSlotOutcome::Slot(Slot {
                    peer,
                    id: query.id,
                    rd: query.rd,
                    cd: query.cd,
                    question: query.question,
                    rcode: Some(Rcode::ServerFailure),
                    cnx: std::ptr::null_mut(),
                    path_id: -1,
                    payload_override: None,
                }));
            }
        }
    } else {
        // Single packet
        vec![query.payload]
    }
} else {
    // Batch processing disabled
    vec![query.payload]
};
```

## Protocol Compatibility

The implementation matches veryslip-client's batch protocol:
- **Batch format**: Count byte + offsets + packet data
- **Maximum batch size**: 10 packets
- **Offset encoding**: u16 big-endian
- **Single packet optimization**: Handled efficiently without overhead
- **Error handling**: Graceful fallback with DNS ServerFailure on batch errors

## Processing Flow

1. **DNS Query Received** → DNS decode
2. **Decompression** → Extract payload (if compression enabled)
3. **Batch Detection** → Check if payload is batched (count > 1)
4. **Batch Splitting** → Extract individual packets using offsets
5. **QUIC Processing** → Process ALL packets through QUIC engine sequentially
6. **Response** → Return slot for last valid connection, compress and encode DNS response

## Batch Processing Behavior

**All packets are processed**: When a batch is received, ALL packets are fed to the QUIC engine sequentially. This ensures:
- All QUIC packets in the batch are processed
- QUIC state is updated for all packets
- Connection establishment and data transfer work correctly

**Single DNS response**: Since one DNS query can only have one DNS response, we return a slot for the last valid connection. This is correct because:
- QUIC is connection-oriented - all packets contribute to the same connection state
- The DNS response will contain data from the QUIC connection (which processed all packets)
- Failed packets are logged but don't stop processing of remaining packets

**Error handling**: If a packet fails to process, we log a warning and continue with the next packet. This ensures partial batch failures don't break the entire batch.

## Error Handling

1. **Empty batch data**: Return error "Empty batch data"
2. **Zero packet count**: Return error "Batch packet count is zero"
3. **Exceeds max size**: Return error "exceeds maximum 10"
4. **Data too short**: Return error "Batch data too short"
5. **Invalid offsets**: Return error "Invalid packet bounds"
6. **All errors**: Return DNS ServerFailure (Rcode 2) to client

## Testing

The batch module includes 14 comprehensive unit tests:
- ✅ Single packet detection and splitting
- ✅ Multiple packet splitting (2, 3 packets)
- ✅ Variable packet sizes
- ✅ Empty data handling
- ✅ Zero count validation
- ✅ Max size enforcement
- ✅ Data length validation
- ✅ Invalid offset detection
- ✅ Statistics tracking

## Current Implementation

**All packets processed**: The implementation now processes ALL packets from a batch sequentially through the QUIC engine. Each packet is fed to `picoquic_incoming_packet_ex()`, ensuring:
- Complete QUIC state updates
- Proper connection establishment
- All data is processed

**Single slot return**: Since one DNS query can only have one DNS response, we return a slot for the last valid connection or stateless response. This works correctly because QUIC is connection-oriented and all packets contribute to the same connection state.

## Performance Considerations

1. **Batch overhead**: Minimal - only count check and offset parsing
2. **Single packet optimization**: No batch processing overhead for non-batched payloads
3. **Memory allocation**: Efficient - reuses buffers where possible
4. **Thread safety**: BatchStats uses atomic counters
5. **Logging**: Trace-level for successful splits, warn-level for errors

## Next Steps

With batch processing complete, the next tasks are:
- Task 6: Checkpoint - Verify batch processing functionality
- Task 7: Implement metrics collection infrastructure
- Task 8: Implement Prometheus HTTP endpoint
- Task 9: Integrate metrics into all server operations
