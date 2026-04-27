//! Integration tests for the QuantumVault full node.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use qv_core::{Block, BlockHash, ProtocolParams, Transaction};
use qv_node::{Node, NodeConfig};

#[tokio::test]
async fn test_node_creation_devnet() {
    let config = NodeConfig::devnet();
    let node = Node::new(config).await.expect("failed to create node");
    assert_eq!(node.config.network, "devnet");
}

#[tokio::test]
async fn test_node_creation_testnet() {
    let config = NodeConfig::testnet();
    let node = Node::new(config).await.expect("failed to create node");
    assert_eq!(node.config.network, "testnet");
}

#[tokio::test]
async fn test_node_creation_mainnet() {
    let config = NodeConfig::mainnet();
    let node = Node::new(config).await.expect("failed to create node");
    assert_eq!(node.config.network, "mainnet");
}

#[tokio::test]
async fn test_node_event_send() {
    let config = NodeConfig::devnet();
    let node = Node::new(config).await.expect("failed to create node");

    let event = qv_node::node::NodeEvent::Shutdown;
    node.send_event(event)
        .await
        .expect("failed to send event");
}

#[tokio::test]
async fn test_node_graceful_shutdown() {
    let config = NodeConfig::devnet();
    let node = Node::new(config).await.expect("failed to create node");
    let event_tx = node.event_tx.clone();

    // Spawn the node in a background task.
    let node_task = tokio::spawn(async move { node.run().await });

    // Give it a moment to start.
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Send a shutdown event.
    event_tx
        .send(qv_node::node::NodeEvent::Shutdown)
        .await
        .expect("failed to send shutdown");

    // Wait for the node to shut down.
    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        node_task,
    )
    .await
    .expect("node shutdown timed out")
    .expect("node task panicked");

    assert!(
        result.is_ok(),
        "node did not shut down cleanly: {:?}",
        result
    );
}

#[tokio::test]
async fn test_config_devnet_preset() {
    let cfg = NodeConfig::devnet();
    assert_eq!(cfg.network, "devnet");
    assert_eq!(cfg.gossip.max_peers, 100);
    assert_eq!(cfg.mempool.max_clear_pool_size, 10000);
    assert_eq!(cfg.storage_backend, "memory");
}

#[tokio::test]
async fn test_config_testnet_preset() {
    let cfg = NodeConfig::testnet();
    assert_eq!(cfg.network, "testnet");
    assert_eq!(cfg.gossip.max_peers, 200);
    assert_eq!(cfg.storage_backend, "redb");
}

#[tokio::test]
async fn test_config_mainnet_preset() {
    let cfg = NodeConfig::mainnet();
    assert_eq!(cfg.network, "mainnet");
    assert_eq!(cfg.gossip.max_peers, 500);
    assert_eq!(cfg.storage_backend, "rocksdb");
}

#[tokio::test]
async fn test_config_for_network() {
    let cfg = NodeConfig::for_network("mainnet", None).expect("failed to get mainnet config");
    assert_eq!(cfg.network, "mainnet");

    let cfg = NodeConfig::for_network("testnet", None).expect("failed to get testnet config");
    assert_eq!(cfg.network, "testnet");

    let cfg = NodeConfig::for_network("devnet", None).expect("failed to get devnet config");
    assert_eq!(cfg.network, "devnet");
}

#[tokio::test]
async fn test_config_validation_fails_empty_network() {
    let mut cfg = NodeConfig::devnet();
    cfg.network.clear();
    assert!(cfg.validate().is_err());
}

#[tokio::test]
async fn test_config_validation_fails_zero_max_peers() {
    let mut cfg = NodeConfig::devnet();
    cfg.gossip.max_peers = 0;
    assert!(cfg.validate().is_err());
}

#[tokio::test]
async fn test_config_validation_fails_zero_mempool_size() {
    let mut cfg = NodeConfig::devnet();
    cfg.mempool.max_clear_pool_size = 0;
    assert!(cfg.validate().is_err());
}

#[tokio::test]
async fn test_cli_args_parse_bootstrap() {
    let args = qv_node::CliArgs {
        config: std::path::PathBuf::from("config/qv-node.toml"),
        data_dir: std::path::PathBuf::from("./data"),
        network: "devnet".to_string(),
        listen: None,
        rpc_addr: "127.0.0.1:8545".parse().unwrap(),
        metrics_addr: "127.0.0.1:9090".parse().unwrap(),
        bootstrap: Some("/ip4/192.168.1.100/tcp/10333/p2p/QmXxxx,/ip4/192.168.1.101/tcp/10333/p2p/QmYyyy".to_string()),
        init: false,
        log_level: "info".to_string(),
    };
    let addrs = args.parse_bootstrap_addrs();
    assert_eq!(addrs.len(), 2);
}

#[test]
fn test_metrics_recording_compiles() {
    // Just ensure metrics functions can be called and compiled.
    qv_node::metrics::record_block_validated();
    qv_node::metrics::record_block_rejected("test");
    qv_node::metrics::record_tx_received();
    qv_node::metrics::record_tx_rejected("test");
    qv_node::metrics::record_gossip_message_in("blocks");
    qv_node::metrics::record_peer_connected();
    qv_node::metrics::record_peer_disconnected();
    qv_node::metrics::record_tip_height(100);
    qv_node::metrics::record_mempool_size(50);
    qv_node::metrics::record_block_validation_time(0.5);
    qv_node::metrics::record_rpc_request_time("qv_getTip", 0.01);
}

#[tokio::test]
async fn test_node_multiple_creation_and_cleanup() {
    for i in 0..3 {
        let config = NodeConfig::devnet();
        let node = Node::new(config).await.expect("failed to create node");
        assert_eq!(node.config.network, "devnet");

        let event_tx = node.event_tx.clone();
        let node_task = tokio::spawn(async move { node.run().await });

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        event_tx
            .send(qv_node::node::NodeEvent::Shutdown)
            .await
            .expect("failed to send shutdown");

        let _result = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            node_task,
        )
        .await
        .expect("iteration {i} timed out")
        .expect("iteration {i} panicked");
    }
}
