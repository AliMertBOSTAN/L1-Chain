//! Prometheus metrics collection and exporter.

use metrics::{counter, gauge, histogram};
use std::net::SocketAddr;

/// Initialize the metrics exporter (Prometheus HTTP endpoint).
///
/// In `metrics-exporter-prometheus` 0.14 the builder takes the listener
/// address up front and `.install()` does both: it registers the global
/// recorder *and* spawns the HTTP server on the current Tokio runtime.
/// (The previously-used `render_http_server` no longer exists.)
pub fn init_exporter(addr: SocketAddr) -> crate::NodeResult<()> {
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_listener(addr)
        .install()
        .map_err(|e| {
            crate::NodeError::Other(format!("failed to install Prometheus exporter: {e}"))
        })?;

    tracing::info!(addr = %addr, "Prometheus metrics exporter listening");
    Ok(())
}

/// Record a block validation event.
pub fn record_block_validated() {
    counter!("blocks_validated").increment(1);
}

/// Record a rejected block.
pub fn record_block_rejected(reason: &str) {
    counter!("blocks_rejected").increment(1);
    counter!("blocks_rejected_reason", "reason" => reason.to_string()).increment(1);
}

/// Record a received transaction.
pub fn record_tx_received() {
    counter!("tx_received").increment(1);
}

/// Record a rejected transaction.
pub fn record_tx_rejected(reason: &str) {
    counter!("tx_rejected").increment(1);
    counter!("tx_rejected_reason", "reason" => reason.to_string()).increment(1);
}

/// Record a gossip message received.
pub fn record_gossip_message_in(topic: &str) {
    counter!("gossip_messages_in", "topic" => topic.to_string()).increment(1);
}

/// Record a connected peer.
pub fn record_peer_connected() {
    gauge!("peers_connected").increment(1.0);
}

/// Record a disconnected peer.
pub fn record_peer_disconnected() {
    gauge!("peers_connected").decrement(1.0);
}

/// Set the current tip block height.
pub fn record_tip_height(height: u64) {
    gauge!("tip_height").set(height as f64);
}

/// Set the current mempool size.
pub fn record_mempool_size(size: usize) {
    gauge!("mempool_size").set(size as f64);
}

/// Record block validation latency.
pub fn record_block_validation_time(secs: f64) {
    histogram!("block_validate_seconds").record(secs);
}

/// Record RPC request latency.
pub fn record_rpc_request_time(method: &str, secs: f64) {
    histogram!("rpc_request_seconds", "method" => method.to_string()).record(secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_functions_compile() {
        // Just verify the functions exist and can be called without error
        record_block_validated();
        record_block_rejected("test");
        record_tx_received();
        record_tx_rejected("test");
        record_gossip_message_in("blocks");
        record_peer_connected();
        record_peer_disconnected();
        record_tip_height(100);
        record_mempool_size(50);
        record_block_validation_time(0.5);
        record_rpc_request_time("qv_getTip", 0.01);
    }
}
