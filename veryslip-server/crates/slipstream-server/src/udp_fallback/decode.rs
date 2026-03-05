use super::{dummy_sockaddr_storage, FallbackManager, PacketContext};
use crate::server::{ServerError, Slot};
use crate::compression::CompressionEngine;
use crate::batch::BatchProcessor;
use slipstream_dns::{decode_query_with_domains, DecodeQueryError, Rcode};
use slipstream_ffi::picoquic::{
    picoquic_cnx_t, picoquic_incoming_packet_ex, picoquic_quic_t, slipstream_disable_ack_delay,
};
use slipstream_ffi::take_stateless_packet_for_cid;
use std::net::SocketAddr;
use std::sync::Arc;

enum DecodeSlotOutcome {
    Slot(Slot),
    DnsOnly,
    Drop,
}

pub(crate) async fn handle_packet(
    slots: &mut Vec<Slot>,
    packet: &[u8],
    peer: SocketAddr,
    context: &PacketContext<'_>,
    fallback_mgr: &mut Option<FallbackManager>,
) -> Result<(), ServerError> {
    if let Some(manager) = fallback_mgr.as_mut() {
        if manager.is_active_fallback_peer(peer) {
            manager.forward_existing(packet, peer).await;
            return Ok(());
        }
    }

    match decode_slot(
        packet,
        peer,
        context.domains,
        context.quic,
        context.current_time,
        context.local_addr_storage,
        context.compression,
        context.batch_processor,
    )? {
        DecodeSlotOutcome::Slot(slot) => {
            if let Some(manager) = fallback_mgr.as_mut() {
                manager.mark_dns(peer);
            }
            slots.push(slot);
        }
        DecodeSlotOutcome::DnsOnly => {
            if let Some(manager) = fallback_mgr.as_mut() {
                manager.mark_dns(peer);
            }
        }
        DecodeSlotOutcome::Drop => {
            if let Some(manager) = fallback_mgr.as_mut() {
                manager.handle_non_dns(packet, peer).await;
            }
        }
    }

    Ok(())
}

fn decode_slot(
    packet: &[u8],
    peer: SocketAddr,
    domains: &[&str],
    quic: *mut picoquic_quic_t,
    current_time: u64,
    local_addr_storage: &libc::sockaddr_storage,
    compression: &Option<Arc<CompressionEngine>>,
    batch_processor: &Option<Arc<BatchProcessor>>,
) -> Result<DecodeSlotOutcome, ServerError> {
    match decode_query_with_domains(packet, domains) {
        Ok(mut query) => {
            // Decompress payload if compression is enabled
            if let Some(engine) = compression {
                match engine.decompress(&query.payload) {
                    Ok(decompressed) => {
                        query.payload = decompressed;
                    }
                    Err(e) => {
                        tracing::warn!("Decompression failed from {}: {}", peer, e);
                        // Return DNS error response
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
            }
            
            // Check if payload is batched and split if needed
            let payloads = if let Some(processor) = batch_processor {
                if processor.is_batched(&query.payload) {
                    match processor.split_batch(&query.payload) {
                        Ok(packets) => {
                            tracing::trace!("Split batch into {} packets from {}", packets.len(), peer);
                            packets
                        }
                        Err(e) => {
                            tracing::warn!("Batch splitting failed from {}: {}", peer, e);
                            // Return DNS error response
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
            
            // Process ALL packets through QUIC
            // We process each packet but only return a slot for the last one
            // This ensures all packets are fed to QUIC engine
            let mut last_cnx: *mut picoquic_cnx_t = std::ptr::null_mut();
            let mut last_path: libc::c_int = -1;
            let mut stateless_response: Option<Vec<u8>> = None;
            
            for (idx, payload) in payloads.iter().enumerate() {
                let mut peer_storage = dummy_sockaddr_storage();
                let mut local_storage = unsafe { std::ptr::read(local_addr_storage) };
                let mut first_cnx: *mut picoquic_cnx_t = std::ptr::null_mut();
                let mut first_path: libc::c_int = -1;
                
                let ret = unsafe {
                    picoquic_incoming_packet_ex(
                        quic,
                        payload.as_ptr() as *mut u8,
                        payload.len(),
                        &mut peer_storage as *mut _ as *mut libc::sockaddr,
                        &mut local_storage as *mut _ as *mut libc::sockaddr,
                        0,
                        0,
                        &mut first_cnx,
                        &mut first_path,
                        current_time,
                    )
                };
                
                if ret < 0 {
                    tracing::warn!("Failed to process QUIC packet {} from batch", idx);
                    continue; // Skip this packet, try next one
                }
                
                // Track the last valid connection
                if !first_cnx.is_null() {
                    last_cnx = first_cnx;
                    last_path = first_path;
                    unsafe {
                        slipstream_disable_ack_delay(first_cnx);
                    }
                } else {
                    // Check for stateless packet response
                    if let Some(response_payload) = unsafe { take_stateless_packet_for_cid(quic, payload) } {
                        if !response_payload.is_empty() {
                            stateless_response = Some(response_payload);
                        }
                    }
                }
            }
            
            // Return slot based on what we got
            if !last_cnx.is_null() {
                // We have a connection - return slot for it
                Ok(DecodeSlotOutcome::Slot(Slot {
                    peer,
                    id: query.id,
                    rd: query.rd,
                    cd: query.cd,
                    question: query.question,
                    rcode: None,
                    cnx: last_cnx,
                    path_id: last_path,
                    payload_override: None,
                }))
            } else if let Some(response_payload) = stateless_response {
                // We have a stateless response - return slot with payload override
                Ok(DecodeSlotOutcome::Slot(Slot {
                    peer,
                    id: query.id,
                    rd: query.rd,
                    cd: query.cd,
                    question: query.question,
                    rcode: None,
                    cnx: std::ptr::null_mut(),
                    path_id: -1,
                    payload_override: Some(response_payload),
                }))
            } else {
                // No connection and no stateless response - DNS only
                Ok(DecodeSlotOutcome::DnsOnly)
            }
        }
        Err(DecodeQueryError::Drop) => Ok(DecodeSlotOutcome::Drop),
        Err(DecodeQueryError::Reply {
            id,
            rd,
            cd,
            question,
            rcode,
        }) => {
            let Some(question) = question else {
                // Treat empty-question queries (QDCOUNT=0) as non-DNS for fallback.
                return Ok(DecodeSlotOutcome::Drop);
            };
            Ok(DecodeSlotOutcome::Slot(Slot {
                peer,
                id,
                rd,
                cd,
                question,
                rcode: Some(rcode),
                cnx: std::ptr::null_mut(),
                path_id: -1,
                payload_override: None,
            }))
        }
    }
}
