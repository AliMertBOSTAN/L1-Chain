//! P2P networking for QuantumVault.
//!
//! - **`peer`**: Peer identity, metadata, reputation, `PeerStore`.
//! - **`message`**: Wire message types (`NetworkMessage`, `Envelope`).
//! - **`transport`**: TCP + Noise + Yamux transport config, `NodeIdentity`.
//! - **`gossip`**: GossipSub topics, dedup cache, `build_gossipsub`.
//! - **`node`**: `NetworkNode` orchestrator (Swarm + Kademlia + GossipSub + rate limiter).

#![forbid(unsafe_code)]

pub mod gossip;
pub mod message;
pub mod node;
pub mod peer;
pub mod transport;

use thiserror::Error;

/// Error type for the networking layer.
#[derive(Debug, Error)]
pub enum NetError {
    /// Transport-level error (TCP, Noise handshake, Yamux, etc.).
    #[error("transport error: {0}")]
    Transport(String),

    /// Configuration error.
    #[error("config error: {0}")]
    Config(String),

    /// Gossip publish / subscribe error.
    #[error("gossip error: {0}")]
    Gossip(String),

    /// Codec / serialization error.
    #[error("codec error: {0}")]
    Codec(String),

    /// Received a message with an unsupported protocol version.
    #[error("unsupported version: got {got}, expected {expected}")]
    UnsupportedVersion { got: u16, expected: u16 },

    /// Message exceeds the maximum permitted size.
    #[error("message too large: {size} bytes (limit {limit})")]
    MessageTooLarge { size: usize, limit: usize },

    /// Rate limit exceeded for a peer.
    #[error("rate limited")]
    RateLimited,
}

/// Result alias for the networking layer.
pub type NetResult<T> = Result<T, NetError>;

// Re-export headline types at crate root for ergonomic imports.
pub use gossip::{GossipConfig, SeenCache};
pub use message::{Envelope, MessageKind, NetworkMessage};
pub use node::{NetEvent, NetworkNode, NodeConfig, RateLimitConfig, RateLimiter};
pub use peer::{ConnectionState, Direction, PeerInfo, PeerStore};
pub use transport::{NodeIdentity, TransportConfig};

// Re-export libp2p types used by downstream crates.
pub use libp2p::{Multiaddr, PeerId};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn net_error_display() {
        let err = NetError::Transport("connection refused".to_owned());
        let s = format!("{err}");
        assert!(s.contains("connection refused"));
    }

    #[test]
    fn net_error_version_mismatch() {
        let err = NetError::UnsupportedVersion {
            got: 99,
            expected: 1,
        };
        let s = format!("{err}");
        assert!(s.contains("99"));
        assert!(s.contains("1"));
    }

    #[test]
    fn net_error_message_too_large() {
        let err = NetError::MessageTooLarge {
            size: 10_000_000,
            limit: 4_194_304,
        };
        let s = format!("{err}");
        assert!(s.contains("10000000"));
    }
}
