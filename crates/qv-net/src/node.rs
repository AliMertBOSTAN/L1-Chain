//! Network node orchestrator.
//!
//! [`NodeConfig`] collects all sub-configs (transport, gossip, rate limiter).
//! [`NetworkNode`] composes the libp2p `Swarm` with Kademlia, GossipSub,
//! Identify, Ping, and a simple token-bucket rate limiter.  The async
//! [`NetworkNode::run`] method drives the event loop.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use libp2p::gossipsub::{self, IdentTopic};
use libp2p::identify;
use libp2p::kad::{self, store::MemoryStore};
use libp2p::ping;
use libp2p::request_response::{self, Message as RrMessage};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, SwarmBuilder};
use qv_crypto::{generate_hybrid_keypair, HybridKeyPair};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::gossip::{self, GossipConfig, SeenCache};
use crate::handshake::{
    build_handshake_behaviour, make_hello, process_ack, respond_to_hello, HandshakeBehaviour,
    SessionRecord, SessionStore, DEFAULT_KEM_LEVEL,
};
use crate::message::{Envelope, NetworkMessage};
use crate::peer::{ConnectionState, PeerInfo, PeerStore};
use crate::transport::{NodeIdentity, TransportConfig, QV_AGENT_VERSION, QV_PROTOCOL_VERSION};
use crate::NetError;

// ---------------------------------------------------------------------------
// Time helpers
// ---------------------------------------------------------------------------

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Rate limiter
// ---------------------------------------------------------------------------

/// Simple token-bucket rate limiter configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum messages per peer per window.
    pub max_per_window: u64,
    /// Window duration in seconds.
    pub window_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_per_window: 100,
            window_secs: 10,
        }
    }
}

/// Per-peer rate limiter state.
#[derive(Debug)]
struct PeerBucket {
    count: u64,
    window_start: Instant,
}

/// Tracks per-peer message rates and rejects peers that exceed the limit.
#[derive(Debug)]
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: HashMap<String, PeerBucket>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    #[must_use]
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: HashMap::new(),
        }
    }

    /// Check whether a message from `peer` should be accepted.
    ///
    /// Returns `true` if the message is within the rate limit.
    pub fn allow(&mut self, peer: &PeerId) -> bool {
        let key = peer.to_string();
        let window = Duration::from_secs(self.config.window_secs);
        let now = Instant::now();

        let bucket = self.buckets.entry(key).or_insert(PeerBucket {
            count: 0,
            window_start: now,
        });

        if now.duration_since(bucket.window_start) >= window {
            bucket.count = 0;
            bucket.window_start = now;
        }

        if bucket.count >= self.config.max_per_window {
            return false;
        }

        bucket.count = bucket.count.saturating_add(1);
        true
    }

    /// Purge expired buckets to free memory.
    pub fn purge_expired(&mut self) {
        let window = Duration::from_secs(self.config.window_secs);
        let now = Instant::now();
        self.buckets
            .retain(|_, b| now.duration_since(b.window_start) < window);
    }
}

// ---------------------------------------------------------------------------
// Node configuration
// ---------------------------------------------------------------------------

/// Aggregate configuration for the network node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Transport (TCP + Noise + Yamux) settings.
    pub transport: TransportConfig,
    /// GossipSub settings.
    pub gossip: GossipConfig,
    /// Rate limiter settings.
    pub rate_limit: RateLimitConfig,
    /// Bootstrap peer addresses to connect to on startup.
    pub bootstrap_peers: Vec<String>,
    /// Kademlia replication factor.
    pub kad_replication: usize,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            transport: TransportConfig::default(),
            gossip: GossipConfig::default(),
            rate_limit: RateLimitConfig::default(),
            bootstrap_peers: Vec::new(),
            kad_replication: 20,
        }
    }
}

impl NodeConfig {
    /// Mainnet preset.
    #[must_use]
    pub fn mainnet() -> Self {
        Self::default()
    }

    /// Testnet preset.
    #[must_use]
    pub fn testnet() -> Self {
        Self {
            transport: TransportConfig::testnet(),
            gossip: GossipConfig::testnet(),
            ..Self::default()
        }
    }

    /// Local ephemeral preset (for tests).
    #[must_use]
    pub fn ephemeral() -> Self {
        Self {
            transport: TransportConfig::ephemeral(),
            gossip: GossipConfig::ephemeral(),
            rate_limit: RateLimitConfig {
                max_per_window: 1000,
                window_secs: 5,
            },
            bootstrap_peers: Vec::new(),
            kad_replication: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Composite behaviour
// ---------------------------------------------------------------------------

/// The combined libp2p [`NetworkBehaviour`] for a QuantumVault node.
#[derive(NetworkBehaviour)]
pub struct QvBehaviour {
    /// GossipSub for block/tx/vrf/vote propagation.
    pub gossipsub: gossipsub::Behaviour,
    /// Kademlia DHT for peer discovery.
    pub kademlia: kad::Behaviour<MemoryStore>,
    /// Identify protocol for exchanging peer metadata.
    pub identify: identify::Behaviour,
    /// Ping for liveness checks.
    pub ping: ping::Behaviour,
    /// Hybrid X25519+Kyber post-quantum handshake (NET-01 / ADR-007).
    /// Runs on top of libp2p's classical Noise authentication; provides a
    /// per-peer post-quantum shared secret deposited into `SessionStore`.
    pub handshake: HandshakeBehaviour,
}

// ---------------------------------------------------------------------------
// Network node
// ---------------------------------------------------------------------------

/// Outbound event emitted by the network layer to the application.
#[derive(Clone, Debug)]
pub enum NetEvent {
    /// A new gossip message was received and validated.
    Message {
        source: PeerId,
        message: NetworkMessage,
    },
    /// A new peer connected.
    PeerConnected(PeerId),
    /// A peer disconnected.
    PeerDisconnected(PeerId),
}

/// The top-level network node.
///
/// Owns the libp2p [`Swarm`], peer store, rate limiter, and deduplication
/// cache.  Call [`run`](Self::run) to start the async event loop.
pub struct NetworkNode {
    /// The libp2p Swarm.
    swarm: Swarm<QvBehaviour>,
    /// Known peers.
    pub peer_store: PeerStore,
    /// Rate limiter.
    rate_limiter: RateLimiter,
    /// Deduplication cache.
    seen_cache: SeenCache,
    /// Channel for outbound events.
    event_tx: mpsc::UnboundedSender<NetEvent>,
    /// Channel for receiving outbound events.
    event_rx: Option<mpsc::UnboundedReceiver<NetEvent>>,
    /// Command channel sender (cloneable handle for external publish requests).
    cmd_tx: mpsc::UnboundedSender<NetworkMessage>,
    /// Command channel receiver (consumed inside `run()`).
    cmd_rx: Option<mpsc::UnboundedReceiver<NetworkMessage>>,
    /// Local hybrid X25519+Kyber keypair used for the post-quantum handshake.
    /// Sealed inside `Arc` because the keypair is referenced both by the
    /// behaviour (to embed in outgoing `Hello`s) and by the local response
    /// path (to `decapsulate_hybrid` inbound ciphertexts).
    local_hybrid_kp: Arc<HybridKeyPair>,
    /// Per-peer session secrets derived from completed handshakes. Cloned
    /// out and exposed via [`NetworkNode::session_store`] so callers can
    /// look up the session key for a given peer without going through the
    /// swarm event loop.
    session_store: SessionStore,
}

impl NetworkNode {
    /// Build a new network node from a config and identity.
    pub fn new(config: NodeConfig, identity: NodeIdentity) -> Result<Self, NetError> {
        let local_peer_id = identity.peer_id();
        let keypair = identity.keypair().clone();

        // Build GossipSub
        let gossipsub_behaviour = gossip::build_gossipsub(&keypair, &config.gossip)?;

        // Build Kademlia
        let kad_store = MemoryStore::new(local_peer_id);
        let kademlia = kad::Behaviour::new(local_peer_id, kad_store);

        // Build Identify
        let identify_config =
            identify::Config::new(QV_PROTOCOL_VERSION.to_owned(), keypair.public())
                .with_agent_version(QV_AGENT_VERSION.to_owned());
        let identify = identify::Behaviour::new(identify_config);

        // Build Ping
        let ping = ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(30)));

        // Build the hybrid handshake behaviour (NET-01).
        let handshake_behaviour = build_handshake_behaviour();

        let behaviour = QvBehaviour {
            gossipsub: gossipsub_behaviour,
            kademlia,
            identify,
            ping,
            handshake: handshake_behaviour,
        };

        let swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )
            .map_err(|e| NetError::Transport(format!("tcp+noise+yamux: {e}")))?
            .with_behaviour(|_| behaviour)
            .map_err(|e| NetError::Transport(format!("behaviour: {e}")))?
            .with_swarm_config(|cfg| {
                cfg.with_idle_connection_timeout(config.transport.idle_timeout())
            })
            .build();

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        // Generate the local hybrid keypair used as the initiator pubkey in
        // every Hello we send out. One keypair per node lifetime: the
        // primitive uses an internal ephemeral X25519 keypair on every
        // `encapsulate_hybrid` call, so reusing the long-lived keypair as
        // the *responder pubkey* is safe.
        let local_hybrid_kp = Arc::new(generate_hybrid_keypair(DEFAULT_KEM_LEVEL).map_err(
            |e| NetError::Transport(format!("hybrid keypair generation failed: {e}")),
        )?);
        let session_store = SessionStore::new();

        Ok(Self {
            swarm,
            peer_store: PeerStore::new(),
            rate_limiter: RateLimiter::new(config.rate_limit),
            seen_cache: SeenCache::from_config(&config.gossip),
            event_tx,
            event_rx: Some(event_rx),
            cmd_tx,
            cmd_rx: Some(cmd_rx),
            local_hybrid_kp,
            session_store,
        })
    }

    /// Cloneable handle to the per-peer session store. Downstream
    /// components consult this to find the post-handshake shared secret
    /// for a given peer.
    #[must_use]
    pub fn session_store(&self) -> SessionStore {
        self.session_store.clone()
    }

    /// Take the event receiver channel (can only be called once).
    pub fn take_event_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<NetEvent>> {
        self.event_rx.take()
    }

    /// Get a clone of the command sender for external publish requests.
    ///
    /// Messages sent on this channel will be published to the gossip network
    /// inside the `run()` event loop. This avoids the need for external code
    /// to hold a mutable reference to `NetworkNode` while `run()` is active.
    #[must_use]
    pub fn command_sender(&self) -> mpsc::UnboundedSender<NetworkMessage> {
        self.cmd_tx.clone()
    }

    /// The local `PeerId`.
    #[must_use]
    pub fn local_peer_id(&self) -> &PeerId {
        self.swarm.local_peer_id()
    }

    /// Start listening on the configured address.
    pub fn listen(&mut self, addr: Multiaddr) -> Result<(), NetError> {
        self.swarm
            .listen_on(addr)
            .map_err(|e| NetError::Transport(format!("listen: {e}")))?;
        Ok(())
    }

    /// Dial a remote peer by multiaddress.
    pub fn dial(&mut self, addr: Multiaddr) -> Result<(), NetError> {
        self.swarm
            .dial(addr)
            .map_err(|e| NetError::Transport(format!("dial: {e}")))?;
        Ok(())
    }

    /// Subscribe to all gossip topics.
    pub fn subscribe_all(&mut self) -> Result<(), NetError> {
        for topic_str in gossip::all_topics() {
            let topic = IdentTopic::new(topic_str);
            self.swarm
                .behaviour_mut()
                .gossipsub
                .subscribe(&topic)
                .map_err(|e| NetError::Config(format!("subscribe {topic_str}: {e}")))?;
        }
        Ok(())
    }

    /// Publish a `NetworkMessage` to the appropriate gossip topic.
    pub fn publish(&mut self, msg: &NetworkMessage) -> Result<(), NetError> {
        let kind = msg.kind();
        let topic_str = gossip::topic_for_kind(kind)
            .ok_or_else(|| NetError::Config(format!("no gossip topic for {kind:?}")))?;

        let wire = Envelope::encode(msg)?;
        let topic = IdentTopic::new(topic_str);

        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(topic, wire)
            .map_err(|e| NetError::Gossip(format!("publish: {e}")))?;

        Ok(())
    }

    /// Run the event loop. This is `async` and should be spawned on a tokio runtime.
    ///
    /// Processes swarm events, updates peer store, applies rate limiting and
    /// deduplication, and emits [`NetEvent`]s on the channel. Also processes
    /// outbound publish commands from the command channel.
    pub async fn run(&mut self) {
        // Take the command receiver (can only be taken once).
        let mut cmd_rx = self.cmd_rx.take().unwrap_or_else(|| {
            let (_tx, rx) = mpsc::unbounded_channel();
            rx
        });

        loop {
            tokio::select! {
                // Outbound publish commands from other tasks.
                Some(msg) = cmd_rx.recv() => {
                    if let Err(e) = self.publish(&msg) {
                        warn!(error = %e, "failed to publish message via command channel");
                    }
                }
                // Inbound swarm events.
                event = self.swarm.select_next_some() => {
                match event {
                SwarmEvent::Behaviour(QvBehaviourEvent::Gossipsub(
                    gossipsub::Event::Message {
                        propagation_source,
                        message_id,
                        message,
                    },
                )) => {
                    // Rate limit
                    if !self.rate_limiter.allow(&propagation_source) {
                        warn!(
                            peer = %propagation_source,
                            "rate limited — dropping gossip message"
                        );
                        continue;
                    }

                    // Deduplication
                    if self.seen_cache.insert(message_id.0.clone()) {
                        debug!(
                            peer = %propagation_source,
                            "duplicate gossip message — skipping"
                        );
                        continue;
                    }

                    // Decode
                    match Envelope::decode(&message.data) {
                        Ok(net_msg) => {
                            let _ = self.event_tx.send(NetEvent::Message {
                                source: propagation_source,
                                message: net_msg,
                            });
                        }
                        Err(e) => {
                            warn!(
                                peer = %propagation_source,
                                error = %e,
                                "failed to decode gossip message"
                            );
                            if let Some(info) = self.peer_store.get_mut(&propagation_source) {
                                info.record_failure();
                            }
                        }
                    }
                }

                SwarmEvent::Behaviour(QvBehaviourEvent::Identify(
                    identify::Event::Received { peer_id, info, .. },
                )) => {
                    debug!(
                        peer = %peer_id,
                        protocol = ?info.protocol_version,
                        agent = ?info.agent_version,
                        "identify received"
                    );

                    // Add discovered addresses to Kademlia
                    for addr in &info.listen_addrs {
                        self.swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, addr.clone());
                    }

                    // Update peer store
                    if let Some(peer_info) = self.peer_store.get_mut(&peer_id) {
                        peer_info.protocol_version = Some(info.protocol_version.clone());
                        peer_info.agent_version = Some(info.agent_version.clone());
                        for addr in &info.listen_addrs {
                            peer_info.add_address(addr.clone());
                        }
                    }
                }

                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    info!(peer = %peer_id, endpoint = ?endpoint, "connected");

                    let is_dialer = endpoint.is_dialer();
                    let mut peer_info = self
                        .peer_store
                        .get(&peer_id)
                        .cloned()
                        .unwrap_or_else(|| PeerInfo::new(peer_id));
                    peer_info.state = ConnectionState::Connected;
                    peer_info.direction = Some(if is_dialer {
                        crate::peer::Direction::Outbound
                    } else {
                        crate::peer::Direction::Inbound
                    });
                    self.peer_store.upsert(peer_info);

                    let _ = self.event_tx.send(NetEvent::PeerConnected(peer_id));

                    // Dialer initiates the hybrid handshake. The
                    // responder side replies inside its `Request` event
                    // arm below; either side that already has a session
                    // for this peer skips re-handshaking.
                    if is_dialer && self.session_store.get(&peer_id).is_none() {
                        let hello = make_hello(
                            self.swarm.local_peer_id(),
                            self.local_hybrid_kp.as_ref(),
                        );
                        let _ = self
                            .swarm
                            .behaviour_mut()
                            .handshake
                            .send_request(&peer_id, hello);
                        debug!(peer = %peer_id, "hybrid handshake: Hello dispatched");
                    }
                }

                SwarmEvent::Behaviour(QvBehaviourEvent::Handshake(
                    request_response::Event::Message { peer, message },
                )) => match message {
                    RrMessage::Request {
                        request, channel, ..
                    } => {
                        match respond_to_hello(&request, self.swarm.local_peer_id()) {
                            Ok((ack, ss, initiator_pid)) => {
                                // Store the responder-side session secret
                                // BEFORE we hand the ack back, so a later
                                // burst of traffic from the same peer can
                                // already look up the session key.
                                self.session_store.insert(
                                    initiator_pid,
                                    SessionRecord {
                                        shared_secret: ss,
                                        completed_at: now_unix_secs(),
                                    },
                                );
                                if self
                                    .swarm
                                    .behaviour_mut()
                                    .handshake
                                    .send_response(channel, ack)
                                    .is_err()
                                {
                                    warn!(
                                        peer = %peer,
                                        "hybrid handshake: response channel already closed"
                                    );
                                }
                                debug!(peer = %peer, "hybrid handshake: Ack sent");
                            }
                            Err(e) => {
                                warn!(peer = %peer, error = %e, "hybrid handshake: rejected Hello");
                                if let Some(info) = self.peer_store.get_mut(&peer) {
                                    info.record_failure();
                                }
                            }
                        }
                    }
                    RrMessage::Response { response, .. } => {
                        match process_ack(
                            &response,
                            self.local_hybrid_kp.as_ref(),
                            self.swarm.local_peer_id(),
                        ) {
                            Ok((ss, responder_pid)) => {
                                self.session_store.insert(
                                    responder_pid,
                                    SessionRecord {
                                        shared_secret: ss,
                                        completed_at: now_unix_secs(),
                                    },
                                );
                                info!(
                                    peer = %peer,
                                    sessions = self.session_store.len(),
                                    "hybrid handshake completed (initiator)"
                                );
                            }
                            Err(e) => {
                                warn!(peer = %peer, error = %e, "hybrid handshake: Ack rejected");
                                if let Some(info) = self.peer_store.get_mut(&peer) {
                                    info.record_failure();
                                }
                            }
                        }
                    }
                },

                SwarmEvent::Behaviour(QvBehaviourEvent::Handshake(
                    request_response::Event::OutboundFailure { peer, error, .. },
                )) => {
                    warn!(peer = %peer, error = ?error, "hybrid handshake: outbound failure");
                    if let Some(info) = self.peer_store.get_mut(&peer) {
                        info.record_failure();
                    }
                }

                SwarmEvent::Behaviour(QvBehaviourEvent::Handshake(
                    request_response::Event::InboundFailure { peer, error, .. },
                )) => {
                    warn!(peer = %peer, error = ?error, "hybrid handshake: inbound failure");
                }

                SwarmEvent::Behaviour(QvBehaviourEvent::Handshake(_)) => {
                    // ResponseSent and any other variants — no-op.
                }

                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    info!(peer = %peer_id, "disconnected");

                    if let Some(info) = self.peer_store.get_mut(&peer_id) {
                        info.state = ConnectionState::Disconnected;
                    }
                    // Drop the post-handshake session: a future connection
                    // to the same peer will re-handshake from scratch,
                    // preventing replay of stale shared secrets.
                    self.session_store.remove(&peer_id);

                    let _ = self.event_tx.send(NetEvent::PeerDisconnected(peer_id));
                }

                SwarmEvent::NewListenAddr { address, .. } => {
                    info!(addr = %address, "listening");
                }

                _ => {}
                } // match event
                } // event select arm
            } // tokio::select!
        } // loop
    } // fn run
} // impl NetworkNode

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use libp2p::identity::Keypair;
    use libp2p::PeerId;

    use super::*;

    fn random_peer_id() -> PeerId {
        let kp = Keypair::generate_ed25519();
        PeerId::from_public_key(&kp.public())
    }

    #[test]
    fn rate_limiter_allows_within_window() {
        let config = RateLimitConfig {
            max_per_window: 3,
            window_secs: 60,
        };
        let mut limiter = RateLimiter::new(config);
        let peer = random_peer_id();

        assert!(limiter.allow(&peer));
        assert!(limiter.allow(&peer));
        assert!(limiter.allow(&peer));
        // 4th should be rejected
        assert!(!limiter.allow(&peer));
    }

    #[test]
    fn rate_limiter_separate_peers() {
        let config = RateLimitConfig {
            max_per_window: 1,
            window_secs: 60,
        };
        let mut limiter = RateLimiter::new(config);
        let p1 = random_peer_id();
        let p2 = random_peer_id();

        assert!(limiter.allow(&p1));
        assert!(!limiter.allow(&p1));

        // p2 has its own bucket
        assert!(limiter.allow(&p2));
        assert!(!limiter.allow(&p2));
    }

    #[test]
    fn node_config_presets() {
        let main = NodeConfig::mainnet();
        let test = NodeConfig::testnet();
        let eph = NodeConfig::ephemeral();

        assert_eq!(main.kad_replication, 20);
        assert!(eph.kad_replication < main.kad_replication);
        assert!(test.transport.max_connections > main.transport.max_connections);
    }

    #[test]
    fn network_node_constructs() {
        let config = NodeConfig::ephemeral();
        let identity = NodeIdentity::generate();
        let node = NetworkNode::new(config, identity);
        assert!(node.is_ok());
    }
}
