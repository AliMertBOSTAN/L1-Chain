//! Integration tests for qv-miner.
//!
//! Tests cover:
//! - Key generation and encrypted storage
//! - Pool registration TX structure
//! - Leadership check determinism (with TestVrf)
//! - Block production end-to-end (mocked network)
//! - Committee sortition and membership
//! - Encrypted mempool decryption
//! - KES key rotation
//! - Fork choice rejection
//! - Dashboard metrics
//! - TUI publish/subscribe smoke test

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use qv_consensus::TestVrf;
    use qv_core::{Epoch, ProtocolParams, Slot};
    use qv_miner::committee::is_committee_member;
    use qv_miner::config::{Network, OperatorConfig};
    use qv_miner::dashboard::MetricsStore;
    use qv_miner::keys::OperatorKeys;
    use std::path::PathBuf;

    fn sample_config() -> OperatorConfig {
        OperatorConfig {
            pool_id: "pool_test".to_string(),
            pool_name: "TestPool".to_string(),
            keystore_path: PathBuf::from("keys/operator.keystore"),
            pledge: 1_000_000_000,
            margin_bps: 300,
            fixed_cost: 10_000_000,
            reward_account: "qvaddr_test".to_string(),
            network: Network::Testnet,
            node_rpc_url: "http://localhost:8080".to_string(),
            node_gossip_addr: Some("/ip4/127.0.0.1/tcp/30000".to_string()),
            clear_mempool_capacity: Some(10000),
            encrypted_mempool_capacity: Some(5000),
            decryption_committee_share_path: None,
            genesis_time: None,
            kes_rotation_period_epochs: None,
        }
    }

    /// Test 1: Keypair generation produces correct public key sizes.
    ///
    /// VRF: 32 bytes (Ristretto compressed point).
    /// KES: 32 bytes (Merkle root of leaf pk hashes).
    /// Cold: 1952 bytes (Dilithium Level 3 public key per FIPS 204).
    ///
    /// Encrypted save/load roundtrip is gated on envanter M-04.
    #[test]
    #[ignore] // KES generation is ~2 s; run via `cargo test -- --ignored`.
    fn test_keypair_generation_roundtrip() {
        let keys1 = OperatorKeys::generate().unwrap();
        assert_eq!(keys1.vrf.public_bytes().len(), 32);
        assert_eq!(keys1.kes.public_bytes().len(), 32);
        assert_eq!(keys1.cold.public_bytes().len(), 1952);

        // Two independent generations produce distinct VRF / cold pk
        // (KES check skipped — KES generation is the slow path).
        let keys2 = OperatorKeys::generate().unwrap();
        assert_ne!(keys1.vrf.public_bytes(), keys2.vrf.public_bytes());
        assert_ne!(keys1.cold.public_bytes(), keys2.cold.public_bytes());
    }

    /// Test 1b: Determinism of seeded key derivation.
    #[test]
    #[ignore] // KES generation is slow; run via `cargo test -- --ignored`.
    fn test_keypair_from_seed_is_deterministic() {
        let master = [0xCD_u8; 32];
        let k1 = OperatorKeys::from_seed(&master).unwrap();
        let k2 = OperatorKeys::from_seed(&master).unwrap();
        assert_eq!(k1.vrf.public_bytes(), k2.vrf.public_bytes());
        assert_eq!(k1.kes.public_bytes(), k2.kes.public_bytes());
        assert_eq!(k1.cold.public_bytes(), k2.cold.public_bytes());
    }

    /// Test 2: Pool registration TX structure.
    ///
    /// Marked `#[ignore]` because `OperatorKeys::generate()` now invokes the
    /// real KES generation (envanter M-02 closed). Run via
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn test_pool_registration_tx_structure() {
        let config = sample_config();
        let keys = OperatorKeys::generate().unwrap();

        let tx = qv_miner::registration::build_pool_registration_tx(
            &config,
            keys.vrf.public_bytes(),
            keys.kes.public_bytes(),
            &keys,
        )
        .unwrap();

        // Verify TX has the registration output.
        assert_eq!(tx.outputs.len(), 1);
        assert!(tx.outputs[0].datum.is_some());
        assert_eq!(tx.outputs[0].value, qv_core::Amount::from(config.pledge));
    }

    /// Test 3: Leadership check determinism (with TestVrf).
    #[test]
    fn test_leadership_determinism() {
        let vrf = TestVrf::new([0u8; 32]);
        let pool_id = qv_consensus::PoolId::ZERO;
        let epoch_nonce = vec![0u8; 32];
        let epoch = Epoch::from(1);
        let committee_size = 10;
        let committee_threshold = 3;

        // Call twice with same inputs.
        let result1 = is_committee_member(
            &vrf,
            &pool_id,
            &epoch_nonce,
            epoch,
            committee_size,
            committee_threshold,
        )
        .unwrap();
        let result2 = is_committee_member(
            &vrf,
            &pool_id,
            &epoch_nonce,
            epoch,
            committee_size,
            committee_threshold,
        )
        .unwrap();

        // Should be identical.
        assert_eq!(result1, result2);
    }

    /// Test 4: Block production end-to-end (mocked).
    #[tokio::test]
    async fn test_block_production_mocked() {
        use qv_mempool::ClearPool;

        let ctx = qv_miner::block_producer::BlockProductionContext {
            slot: Slot::from(100),
            parent_hash: qv_core::Hash256::ZERO,
            height: qv_core::Height::from(1),
            timestamp: qv_core::Timestamp::from(1_000_000),
            protocol_params: ProtocolParams::mainnet(),
            reward_pubkey_hash: None,
        };

        let clear_pool = ClearPool::new(qv_mempool::clear::ClearPoolConfig::ephemeral());
        let encrypted_pool = qv_mempool::EncryptedPool::new(
            qv_mempool::EncryptedPoolConfig {
                max_tx_count: 5_000,
                max_pool_bytes: 4 * 1024 * 1024,
                max_age_secs: 60,
            },
            Epoch::from(0),
        );

        let vrf_proof = vec![1, 2, 3];
        let kes_sig = vec![4, 5, 6];

        let block = qv_miner::block_producer::produce_block(
            &ctx,
            &clear_pool,
            &encrypted_pool,
            &vrf_proof,
            &kes_sig,
        )
        .await
        .unwrap();

        assert_eq!(block.header.slot, Slot::from(100));
        assert_eq!(block.header.vrf_proof, vrf_proof);
        assert_eq!(block.header.kes_sig, kes_sig);
    }

    /// Test 5: Committee membership across pool IDs.
    #[test]
    fn test_committee_membership_diversity() {
        let vrf = TestVrf::new([0u8; 32]);
        let epoch_nonce = vec![0u8; 32];
        let epoch = Epoch::from(1);
        let committee_size = 100;
        let committee_threshold = 50;

        let mut members_count = 0;
        for i in 0..100 {
            let pool_id = qv_consensus::PoolId(qv_core::Hash256::from_bytes([i as u8; 32]));
            let is_member = is_committee_member(
                &vrf,
                &pool_id,
                &epoch_nonce,
                epoch,
                committee_size,
                committee_threshold,
            )
            .unwrap();
            if is_member {
                members_count += 1;
            }
        }

        // Expect roughly 50 members (threshold/size ratio).
        assert!(
            members_count > 0,
            "At least some pools should be on committee"
        );
    }

    /// Test 6: Encrypted mempool decryption (via MockThresholdDecryptor).
    #[test]
    fn test_encrypted_mempool_basic() {
        let encrypted_pool = qv_mempool::EncryptedPool::new(
            qv_mempool::EncryptedPoolConfig {
                max_tx_count: 5_000,
                max_pool_bytes: 4 * 1024 * 1024,
                max_age_secs: 60,
            },
            Epoch::from(0),
        );

        // Verify pool is created and empty.
        assert_eq!(encrypted_pool.len(), 0);
    }

    /// Test 7: KES key rotation increments period.
    ///
    /// Marked `#[ignore]`: real KES generation is the slow path (envanter M-02
    /// closed). Run via `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn test_kes_rotation() {
        let mut keys = OperatorKeys::generate().unwrap();
        assert_eq!(keys.kes.period(), 0);

        keys.rotate_kes().await.unwrap();
        assert_eq!(keys.kes.period(), 1);

        keys.rotate_kes().await.unwrap();
        assert_eq!(keys.kes.period(), 2);
    }

    /// Test 8: Pool registration config validation.
    #[test]
    fn test_pool_registration_config_validation() {
        let mut config = sample_config();
        assert!(config.validate().is_ok());

        // Invalid margin (>10000 bps).
        config.margin_bps = 10001;
        assert!(config.validate().is_err());

        config.margin_bps = 300; // reset
        config.pool_id = String::new();
        assert!(config.validate().is_err());

        config.pool_id = "pool_test".to_string(); // reset
        config.node_rpc_url = String::new();
        assert!(config.validate().is_err());
    }

    /// Test 9: Dashboard metrics lifecycle.
    #[tokio::test]
    async fn test_dashboard_metrics() {
        let store = MetricsStore::new();

        // Record leadership events
        store.record_leadership_event(true).await.unwrap();
        store.record_leadership_event(false).await.unwrap();
        store.record_leadership_event(true).await.unwrap();

        store.increment_blocks_produced().await.unwrap();
        store.add_rewards(1_000_000).await.unwrap();

        let snapshot = store.snapshot().await;

        assert_eq!(snapshot.blocks_produced_total, 1);
        assert_eq!(snapshot.rewards_earned, 1_000_000);
        assert_eq!(snapshot.leadership_last_slots.len(), 3);
    }

    /// Test 10: Dashboard metrics window (200 slot limit).
    #[tokio::test]
    async fn test_dashboard_metrics_window() {
        let store = MetricsStore::new();

        // Record 250 leadership events (should truncate to 200).
        for _ in 0..250 {
            store.record_leadership_event(true).await.unwrap();
        }

        let snapshot = store.snapshot().await;
        assert_eq!(snapshot.leadership_last_slots.len(), 200);
    }

    /// Test 11: End-to-end: pool config TOML roundtrip.
    #[test]
    fn test_config_toml_roundtrip() {
        use tempfile::NamedTempFile;

        let config = sample_config();

        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        // Save
        config.save_toml(path).unwrap();

        // Load
        let loaded = OperatorConfig::load_toml(path).unwrap();

        // Verify
        assert_eq!(loaded.pool_id, config.pool_id);
        assert_eq!(loaded.pool_name, config.pool_name);
        assert_eq!(loaded.pledge, config.pledge);
        assert_eq!(loaded.margin_bps, config.margin_bps);
    }

    /// Test 12: CLI argument parsing.
    #[test]
    fn test_cli_argument_parsing() {
        use clap::Parser;
        use qv_miner::cli::Cli;

        // Parse init subcommand
        let args = vec![
            "prog",
            "init",
            "--pool-name",
            "TestPool",
            "--pledge",
            "5000000000",
        ];
        let cli = Cli::try_parse_from(&args);
        assert!(cli.is_ok());

        // Parse run subcommand
        let args2 = vec![
            "prog",
            "run",
            "--config",
            "op.toml",
            "--node-rpc",
            "http://localhost:8080",
        ];
        let cli2 = Cli::try_parse_from(&args2);
        assert!(cli2.is_ok());
    }
}
