//! Integration tests for `qv-net`.
//!
//! These tests exercise cross-module interactions: message encode/decode,
//! gossip topic routing, peer store lifecycle, rate limiting, dedup cache,
//! and network node construction.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::time::Duration;

use libp2p::identity::Keypair;
use libp2p::PeerId;

use qv_core::{
    Amount, Block, BlockHash, BlockHeader, Height, OutPoint, Script, Transaction, TxId, TxInput,
    TxOutput,
};
use qv_net::gossip::{self, SeenCache};
use qv_net::message::*;
use qv_net::node::{NetworkNode, NodeConfig, RateLimitConfig, RateLimiter};
use qv_net::peer::*;
use qv_net::transport::{NodeIdentity, TransportConfig};
use qv_net::NetError;

fn random_peer_id() -> PeerId {
    let kp = Keypair::generate_ed25519();
    PeerId::from_public_key(&kp.public())
}

fn make_block(height: u64, marker: u8) -> Block {
    let tx = Transaction::new(
        vec![TxInput::new(OutPoint::new(
            TxId::from_bytes([marker; 32]),
            0,
        ))],
        vec![TxOutput::new(
            Amount::from_smallest_units(100),
            Script::new(vec![marker]),
        )],
    );
    let mut header = BlockHeader::genesis_template();
    header.height = Height::from(height);
    header.prev_hash = BlockHash::from_bytes([marker; 32]);
    let mut block = Block::new(header, vec![tx]);
    block.header.merkle_root = block.compute_merkle_root().unwrap();
    block
}

// ---------------------------------------------------------------------------
// 1) Full block message encode → decode roundtrip
// ---------------------------------------------------------------------------

#[test]
fn block_message_roundtrip() {
    let block = make_block(42, 0xAA);
    let msg = NetworkMessage::Block(Box::new(block.clone()));

    assert!(msg.is_gossip());
    assert!(!msg.is_request_response());
    assert_eq!(msg.kind(), MessageKind::Block);

    let wire = Envelope::encode(&msg).unwrap();
    let decoded = Envelope::decode(&wire).unwrap();

    match decoded {
        NetworkMessage::Block(b) => assert_eq!(*b, block),
        other => panic!("expected Block, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2) Transaction message roundtrip
// ---------------------------------------------------------------------------

#[test]
fn transaction_message_roundtrip() {
    let tx = Transaction::new(
        vec![TxInput::new(OutPoint::new(TxId::from_bytes([0xBB; 32]), 0))],
        vec![TxOutput::new(
            Amount::from_smallest_units(50),
            Script::new(vec![0xBB]),
        )],
    );

    let msg = NetworkMessage::Transaction(Box::new(tx.clone()));
    assert!(msg.is_gossip());

    let wire = Envelope::encode(&msg).unwrap();
    let decoded = Envelope::decode(&wire).unwrap();

    match decoded {
        NetworkMessage::Transaction(t) => assert_eq!(*t, tx),
        other => panic!("expected Transaction, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 3) Topic routing: gossip messages have topics, req/resp do not
// ---------------------------------------------------------------------------

#[test]
fn gossip_topic_routing() {
    assert!(gossip::topic_for_kind(MessageKind::Block).is_some());
    assert!(gossip::topic_for_kind(MessageKind::Transaction).is_some());
    assert!(gossip::topic_for_kind(MessageKind::VrfProof).is_some());
    assert!(gossip::topic_for_kind(MessageKind::Vote).is_some());

    assert!(gossip::topic_for_kind(MessageKind::GetHeaders).is_none());
    assert!(gossip::topic_for_kind(MessageKind::Headers).is_none());
    assert!(gossip::topic_for_kind(MessageKind::GetBlocks).is_none());
    assert!(gossip::topic_for_kind(MessageKind::Ping).is_none());
    assert!(gossip::topic_for_kind(MessageKind::Pong).is_none());
}

// ---------------------------------------------------------------------------
// 4) Peer lifecycle: discover → connect → interact → ban → evict
// ---------------------------------------------------------------------------

#[test]
fn peer_lifecycle() {
    let mut store = PeerStore::new();
    let pid = random_peer_id();

    // Discover
    let mut info = PeerInfo::new(pid);
    info.add_address("/ip4/1.2.3.4/tcp/9944".parse().unwrap());
    store.upsert(info);
    assert_eq!(store.len(), 1);

    // Connect
    let peer = store.get_mut(&pid).unwrap();
    peer.state = ConnectionState::Connected;
    assert_eq!(store.connected().len(), 1);

    // Successful interactions
    let peer = store.get_mut(&pid).unwrap();
    for _ in 0..5 {
        peer.record_success();
    }
    assert_eq!(peer.successful_interactions, 5);
    assert_eq!(peer.reputation, 5);

    // Many failures → ban
    for _ in 0..15 {
        store.get_mut(&pid).unwrap().record_failure();
    }
    let peer = store.get(&pid).unwrap();
    assert!(peer.is_banned());

    // Evict banned
    let evicted = store.evict_banned();
    assert_eq!(evicted, 1);
    assert!(store.is_empty());
}

// ---------------------------------------------------------------------------
// 5) Rate limiter: multiple peers, window reset semantics
// ---------------------------------------------------------------------------

#[test]
fn rate_limiter_multi_peer() {
    let config = RateLimitConfig {
        max_per_window: 5,
        window_secs: 60,
    };
    let mut limiter = RateLimiter::new(config);

    let p1 = random_peer_id();
    let p2 = random_peer_id();

    for _ in 0..5 {
        assert!(limiter.allow(&p1));
    }
    assert!(!limiter.allow(&p1)); // p1 exhausted

    // p2 still has budget
    for _ in 0..5 {
        assert!(limiter.allow(&p2));
    }
    assert!(!limiter.allow(&p2)); // p2 exhausted
}

// ---------------------------------------------------------------------------
// 6) Dedup cache: insert → duplicate → capacity eviction
// ---------------------------------------------------------------------------

#[test]
fn dedup_cache_behavior() {
    let mut cache = SeenCache::new(Duration::from_secs(60), 5);

    // Fresh entry
    assert!(!cache.insert(vec![1, 2, 3]));
    // Duplicate
    assert!(cache.insert(vec![1, 2, 3]));

    // Fill to capacity
    cache.insert(vec![4]);
    cache.insert(vec![5]);
    cache.insert(vec![6]);
    cache.insert(vec![7]);
    assert_eq!(cache.len(), 5);

    // Overflow → evicts oldest
    cache.insert(vec![8]);
    assert_eq!(cache.len(), 5);
}

// ---------------------------------------------------------------------------
// 7) Transport config validation
// ---------------------------------------------------------------------------

#[test]
fn transport_config_presets_valid() {
    let configs = [
        TransportConfig::mainnet(),
        TransportConfig::testnet(),
        TransportConfig::ephemeral(),
    ];

    for cfg in &configs {
        assert!(cfg.listen_multiaddr().is_ok());
        assert!(cfg.idle_timeout() > Duration::ZERO);
    }
}

// ---------------------------------------------------------------------------
// 8) Node config presets compile and differ
// ---------------------------------------------------------------------------

#[test]
fn node_config_presets_differ() {
    let main = NodeConfig::mainnet();
    let test = NodeConfig::testnet();
    let eph = NodeConfig::ephemeral();

    // Replication factors should differ
    assert!(main.kad_replication > eph.kad_replication);
    // Gossip mesh sizes should differ
    assert!(main.gossip.mesh_n > eph.gossip.mesh_n);
    // Testnet should have more connections
    assert!(test.transport.max_connections > main.transport.max_connections);
}

// ---------------------------------------------------------------------------
// 9) Version mismatch in envelope is rejected
// ---------------------------------------------------------------------------

#[test]
fn version_mismatch_envelope() {
    let msg = NetworkMessage::Ping(PingMsg { nonce: 1 });
    let payload = bincode::serialize(&msg).unwrap();

    let bad_envelope = Envelope {
        version: 255,
        payload,
    };
    let wire = bincode::serialize(&bad_envelope).unwrap();

    let err = Envelope::decode(&wire).unwrap_err();
    assert!(matches!(err, NetError::UnsupportedVersion { got: 255, .. }));
}

// ---------------------------------------------------------------------------
// 10) NetworkNode construction succeeds with ephemeral config
// ---------------------------------------------------------------------------

#[test]
fn network_node_ephemeral_construction() {
    let config = NodeConfig::ephemeral();
    let identity = NodeIdentity::generate();
    let pid = identity.peer_id();

    let node = NetworkNode::new(config, identity).unwrap();
    assert_eq!(*node.local_peer_id(), pid);
}

// ---------------------------------------------------------------------------
// 11) All message types encode/decode without panic
// ---------------------------------------------------------------------------

#[test]
fn all_message_types_roundtrip() {
    let block = make_block(1, 0x01);
    let tx = Transaction::new(
        vec![TxInput::new(OutPoint::new(TxId::from_bytes([0x02; 32]), 0))],
        vec![TxOutput::new(
            Amount::from_smallest_units(10),
            Script::new(vec![0x02]),
        )],
    );

    let messages: Vec<NetworkMessage> = vec![
        NetworkMessage::Block(Box::new(block)),
        NetworkMessage::Transaction(Box::new(tx)),
        NetworkMessage::VrfProof(VrfProofMsg {
            slot: 99,
            vrf_output: vec![1; 32],
            vrf_proof: vec![2; 64],
            producer_key_hash: [0xAA; 32],
        }),
        NetworkMessage::Vote(VoteMsg {
            slot: 50,
            block_hash: BlockHash::from_bytes([0xCC; 32]),
            voter_key_hash: [0xDD; 32],
            signature: vec![3; 48],
        }),
        NetworkMessage::GetHeaders(GetHeadersMsg {
            locator_hashes: vec![BlockHash::from_bytes([0x11; 32])],
            stop_hash: BlockHash::ZERO,
        }),
        NetworkMessage::Headers(HeadersMsg {
            headers: vec![BlockHeader::genesis_template()],
        }),
        NetworkMessage::GetBlocks(GetBlocksMsg {
            hashes: vec![BlockHash::from_bytes([0x22; 32])],
        }),
        NetworkMessage::Ping(PingMsg { nonce: 42 }),
        NetworkMessage::Pong(PongMsg { nonce: 42 }),
    ];

    for msg in &messages {
        let wire = Envelope::encode(msg).unwrap();
        let decoded = Envelope::decode(&wire).unwrap();
        assert_eq!(decoded.kind(), msg.kind());
    }
}

// ---------------------------------------------------------------------------
// 12) PeerStore address dedup + upsert merge
// ---------------------------------------------------------------------------

#[test]
fn peer_store_upsert_merges_addresses() {
    let mut store = PeerStore::new();
    let pid = random_peer_id();

    let mut info1 = PeerInfo::new(pid);
    info1.add_address("/ip4/1.2.3.4/tcp/9944".parse().unwrap());
    store.upsert(info1);

    let mut info2 = PeerInfo::new(pid);
    info2.add_address("/ip4/5.6.7.8/tcp/9944".parse().unwrap());
    info2.state = ConnectionState::Connected;
    store.upsert(info2);

    let peer = store.get(&pid).unwrap();
    assert_eq!(peer.addresses.len(), 2);
    assert_eq!(peer.state, ConnectionState::Connected);
}
