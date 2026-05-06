//! Transport stack: TCP + Noise (XX handshake) + Yamux multiplexing.
//!
//! Future work: replace or augment Noise with a hybrid-KEM handshake
//! (X25519 + Kyber) once `snow` / `rust-libp2p` expose a pluggable KEM slot.
//! For now the Noise-XX handshake with X25519 provides classical security
//! while the PQC upgrade path is documented in CLAUDE.md.

use std::time::Duration;

use libp2p::identity::Keypair;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};

use crate::NetError;

/// Configuration for the transport layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Multiaddress to listen on (e.g. `/ip4/0.0.0.0/tcp/9944`).
    pub listen_addr: String,
    /// Connection idle timeout in seconds.
    pub idle_timeout_secs: u64,
    /// Maximum number of established connections.
    pub max_connections: u32,
    /// Maximum number of pending (not-yet-established) connections.
    pub max_pending: u32,
    /// Dial concurrency factor — how many simultaneous outbound dials.
    pub dial_concurrency: u8,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            listen_addr: "/ip4/0.0.0.0/tcp/9944".to_owned(),
            idle_timeout_secs: 120,
            max_connections: 128,
            max_pending: 64,
            dial_concurrency: 8,
        }
    }
}

impl TransportConfig {
    /// Default configuration for mainnet.
    #[must_use]
    pub fn mainnet() -> Self {
        Self::default()
    }

    /// Configuration with relaxed limits for testnets.
    #[must_use]
    pub fn testnet() -> Self {
        Self {
            listen_addr: "/ip4/0.0.0.0/tcp/19944".to_owned(),
            idle_timeout_secs: 300,
            max_connections: 256,
            max_pending: 128,
            dial_concurrency: 16,
        }
    }

    /// Configuration for local-only development / unit tests.
    #[must_use]
    pub fn ephemeral() -> Self {
        Self {
            listen_addr: "/ip4/127.0.0.1/tcp/0".to_owned(),
            idle_timeout_secs: 30,
            max_connections: 16,
            max_pending: 8,
            dial_concurrency: 4,
        }
    }

    /// Parse `listen_addr` into a `Multiaddr`.
    pub fn listen_multiaddr(&self) -> Result<Multiaddr, NetError> {
        self.listen_addr
            .parse()
            .map_err(|e| NetError::Config(format!("invalid listen_addr: {e}")))
    }

    /// Idle timeout as a `Duration`.
    #[must_use]
    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.idle_timeout_secs)
    }
}

/// Wrapper around a libp2p `Keypair`, providing convenient identity access.
#[derive(Clone)]
pub struct NodeIdentity {
    keypair: Keypair,
}

impl NodeIdentity {
    /// Generate a fresh Ed25519 identity.
    #[must_use]
    pub fn generate() -> Self {
        Self {
            keypair: Keypair::generate_ed25519(),
        }
    }

    /// Wrap an existing keypair.
    #[must_use]
    pub fn from_keypair(keypair: Keypair) -> Self {
        Self { keypair }
    }

    /// The local `PeerId` derived from the public key.
    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        PeerId::from_public_key(&self.keypair.public())
    }

    /// Reference to the underlying `Keypair` (used when building the Swarm).
    #[must_use]
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }
}

impl core::fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "NodeIdentity({})", self.peer_id())
    }
}

/// QuantumVault protocol identification string used by the Identify protocol.
pub const QV_PROTOCOL_VERSION: &str = "/quantumvault/1.0.0";

/// QuantumVault user-agent string.
pub const QV_AGENT_VERSION: &str = "quantumvault-node/0.1.0";

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn transport_config_defaults() {
        let cfg = TransportConfig::default();
        assert_eq!(cfg.max_connections, 128);
        assert_eq!(cfg.idle_timeout_secs, 120);

        let addr = cfg.listen_multiaddr().unwrap();
        assert!(addr.to_string().contains("9944"));
    }

    #[test]
    fn transport_config_presets() {
        let mainnet = TransportConfig::mainnet();
        let testnet = TransportConfig::testnet();
        let ephemeral = TransportConfig::ephemeral();

        assert!(mainnet.listen_multiaddr().unwrap().to_string().contains("9944"));
        assert!(testnet.listen_multiaddr().unwrap().to_string().contains("19944"));
        // ephemeral uses port 0
        assert!(ephemeral.listen_multiaddr().unwrap().to_string().contains("/tcp/0"));
    }

    #[test]
    fn invalid_listen_addr_rejected() {
        let cfg = TransportConfig {
            listen_addr: "not-a-multiaddr".to_owned(),
            ..Default::default()
        };
        assert!(cfg.listen_multiaddr().is_err());
    }

    #[test]
    fn node_identity_peer_id_stable() {
        let id = NodeIdentity::generate();
        let pid1 = id.peer_id();
        let pid2 = id.peer_id();
        assert_eq!(pid1, pid2);
    }

    #[test]
    fn node_identity_from_keypair() {
        let kp = Keypair::generate_ed25519();
        let expected_pid = PeerId::from_public_key(&kp.public());
        let id = NodeIdentity::from_keypair(kp);
        assert_eq!(id.peer_id(), expected_pid);
    }
}
