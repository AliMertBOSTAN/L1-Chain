//! Hybrid X25519 + Kyber post-quantum handshake (envanter NET-01 / ADR-007).
//!
//! # Overview
//!
//! `qv-net` runs an **application-layer** post-quantum handshake on top of
//! libp2p's existing classical transport (TCP + Noise + Yamux). The goal
//! is to defeat "harvest-now-decrypt-later" attackers: even if the
//! attacker breaks the X25519 leg in the future, they still need to break
//! Kyber to recover the session secret.
//!
//! ## Wire protocol
//!
//! A single round trip on the libp2p substream
//! `/quantumvault/handshake/1.0.0`:
//!
//! 1. Initiator sends `HandshakeHello { version, peer_id, hybrid_pk_init }`.
//! 2. Responder
//!     - parses the hello,
//!     - calls [`encapsulate_hybrid`] against `hybrid_pk_init`,
//!     - derives `session_binding = SHA3-256(ss || init_pid || resp_pid)`,
//!     - sends `HandshakeAck { version, peer_id, ciphertext, session_binding }`.
//! 3. Initiator
//!     - parses the ack,
//!     - decapsulates the ciphertext to recover the same `ss`,
//!     - recomputes `session_binding` locally,
//!     - constant-time compares against the received binding and rejects on
//!       mismatch.
//!
//! Both sides retain `ss` as the per-peer session secret. The transcript
//! KDF inside `qv_crypto::hybrid_kem` already binds the ECDH-half and the
//! Kyber-half plus the public-key transcript; we add peer-id binding on
//! top so two parallel sessions to different peers can never collide.
//!
//! ## Threat model (summary; full ADR-007 in `docs/ADR/`)
//!
//! - Active MITM defeated by libp2p's existing Noise-XX authentication
//!   (the peer's static X25519 key is bound to its `PeerId`). This module
//!   does **not** re-authenticate identity — it adds a PQC-secure shared
//!   secret on top.
//! - Harvest-now-decrypt-later defeated by Kyber768 (ML-KEM Level 3) so
//!   long as Kyber survives.
//! - Replay protection comes from the freshness of `hybrid_pk_init` —
//!   each new substream uses a freshly generated keypair.
//!
//! ## Integration
//!
//! The protocol is implemented as a libp2p `request_response::Behaviour`
//! with a bincode codec (see [`HandshakeCodec`]). The owning component
//! attaches the behaviour to its swarm, runs `initiate(peer_id)` once per
//! new connection, and on inbound requests automatically responds.
//! Successful handshakes deposit a [`SessionRecord`] into the per-peer
//! [`SessionStore`].

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::request_response::Codec;
use libp2p::PeerId;
use libp2p::StreamProtocol;
use qv_crypto::{
    decapsulate_hybrid, encapsulate_hybrid, sha3_256, HybridCiphertext, HybridKeyPair,
    HybridPublicKey, KyberLevel, SharedSecret,
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::{NetError, NetResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// libp2p sub-stream protocol name for the hybrid handshake.
pub const HANDSHAKE_PROTOCOL_NAME: &str = "/quantumvault/handshake/1.0.0";

/// Default Kyber parameter set. Level 3 = ML-KEM-768 (~classical 192-bit
/// equivalent). Sized for the same security band as Dilithium L3.
pub const DEFAULT_KEM_LEVEL: KyberLevel = KyberLevel::Level3;

/// Wire format version. Bumped on any breaking change to the structures
/// below; receivers reject any other value.
pub const HANDSHAKE_VERSION: u16 = 1;

/// Maximum permitted bincode size for either handshake message. Kyber768
/// public keys are ~1184 bytes and ciphertexts ~1088 bytes; we round up
/// generously to leave headroom for serde overhead.
pub const MAX_HANDSHAKE_BYTES: usize = 8 * 1024;

/// Domain-separation tag mixed into [`compute_session_binding`].
pub const SESSION_BINDING_TAG: &[u8] = b"qv-hybrid-handshake-binding-v1";

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// Initiator-to-responder message.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeHello {
    /// Wire format version (must equal [`HANDSHAKE_VERSION`]).
    pub version: u16,
    /// Initiator's libp2p `PeerId`, serialised via `PeerId::to_bytes()`.
    pub initiator_peer_id: Vec<u8>,
    /// Initiator's hybrid public key (X25519 + Kyber).
    pub hybrid_pk: HybridPublicKey,
}

/// Responder-to-initiator message.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeAck {
    /// Wire format version (must equal [`HANDSHAKE_VERSION`]).
    pub version: u16,
    /// Responder's libp2p `PeerId`.
    pub responder_peer_id: Vec<u8>,
    /// Hybrid ciphertext encapsulated against the initiator's pubkey.
    pub ciphertext: HybridCiphertext,
    /// `SHA3-256(SESSION_BINDING_TAG || ss || init_pid || resp_pid)`.
    /// Lets the initiator verify in constant time that the responder
    /// actually held the right secrets.
    pub session_binding: [u8; 32],
}

// `HybridPublicKey`, `HybridCiphertext`, and `KyberLevel` derive
// `serde::Serialize` / `serde::Deserialize` upstream in `qv_crypto`
// (added together with this module). No local serde adapters needed.

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Domain-separated session-binding hash.
///
/// `binding = SHA3-256(SESSION_BINDING_TAG || ss || init_pid_bytes || resp_pid_bytes)`.
///
/// Both initiator and responder compute the same value once they agree
/// on `ss`. A constant-time compare detects a wrong-secret responder or
/// any in-flight tampering with the ciphertext.
#[must_use]
pub fn compute_session_binding(
    ss: &SharedSecret,
    initiator_pid: &PeerId,
    responder_pid: &PeerId,
) -> [u8; 32] {
    let init_bytes = initiator_pid.to_bytes();
    let resp_bytes = responder_pid.to_bytes();
    let mut preimage = Vec::with_capacity(
        SESSION_BINDING_TAG.len() + 32 + init_bytes.len() + resp_bytes.len(),
    );
    preimage.extend_from_slice(SESSION_BINDING_TAG);
    preimage.extend_from_slice(ss.as_bytes());
    preimage.extend_from_slice(&init_bytes);
    preimage.extend_from_slice(&resp_bytes);
    sha3_256(&preimage)
}

// ---------------------------------------------------------------------------
// Pure protocol functions
// ---------------------------------------------------------------------------

/// Initiator side. Produce the `Hello` message to send.
#[must_use]
pub fn make_hello(local_peer: &PeerId, local_kp: &HybridKeyPair) -> HandshakeHello {
    HandshakeHello {
        version: HANDSHAKE_VERSION,
        initiator_peer_id: local_peer.to_bytes(),
        hybrid_pk: local_kp.public.clone(),
    }
}

/// Responder side. Process a `Hello`, generate an `Ack` and the shared
/// secret to retain.
///
/// # Errors
/// * `NetError::UnsupportedVersion` — version field mismatch.
/// * `NetError::Codec` — malformed initiator peer id or ciphertext.
pub fn respond_to_hello(
    hello: &HandshakeHello,
    local_peer: &PeerId,
) -> NetResult<(HandshakeAck, SharedSecret, PeerId)> {
    if hello.version != HANDSHAKE_VERSION {
        return Err(NetError::UnsupportedVersion {
            got: hello.version,
            expected: HANDSHAKE_VERSION,
        });
    }
    let init_pid = PeerId::from_bytes(&hello.initiator_peer_id)
        .map_err(|e| NetError::Codec(format!("invalid initiator peer id: {e}")))?;

    let (ciphertext, ss) = encapsulate_hybrid(&hello.hybrid_pk)
        .map_err(|e| NetError::Codec(format!("encapsulate failed: {e}")))?;
    let binding = compute_session_binding(&ss, &init_pid, local_peer);

    Ok((
        HandshakeAck {
            version: HANDSHAKE_VERSION,
            responder_peer_id: local_peer.to_bytes(),
            ciphertext,
            session_binding: binding,
        },
        ss,
        init_pid,
    ))
}

/// Initiator side. Process an `Ack` and decapsulate to the shared secret.
/// Verifies the `session_binding` in constant time.
///
/// # Errors
/// * `NetError::UnsupportedVersion` — version field mismatch.
/// * `NetError::Codec` — malformed responder peer id or ciphertext.
pub fn process_ack(
    ack: &HandshakeAck,
    local_kp: &HybridKeyPair,
    local_peer: &PeerId,
) -> NetResult<(SharedSecret, PeerId)> {
    if ack.version != HANDSHAKE_VERSION {
        return Err(NetError::UnsupportedVersion {
            got: ack.version,
            expected: HANDSHAKE_VERSION,
        });
    }
    let resp_pid = PeerId::from_bytes(&ack.responder_peer_id)
        .map_err(|e| NetError::Codec(format!("invalid responder peer id: {e}")))?;

    let ss = decapsulate_hybrid(local_kp, &ack.ciphertext)
        .map_err(|e| NetError::Codec(format!("decapsulate failed: {e}")))?;

    let expected = compute_session_binding(&ss, local_peer, &resp_pid);
    // `subtle::ConstantTimeEq` is implemented for `[u8]` (slice); arrays
    // coerce via `&[u8]`. Comparing the slice form keeps the check
    // constant-time across all 32 bytes.
    if expected[..].ct_eq(&ack.session_binding[..]).unwrap_u8() != 1 {
        return Err(NetError::Codec(
            "session binding mismatch: peer did not derive the expected shared secret".into(),
        ));
    }

    Ok((ss, resp_pid))
}

// ---------------------------------------------------------------------------
// Session store
// ---------------------------------------------------------------------------

/// Information retained about one completed handshake.
#[derive(Clone)]
pub struct SessionRecord {
    /// The post-handshake shared secret. Bytes are zeroed on drop.
    pub shared_secret: SharedSecret,
    /// Unix-epoch seconds the handshake completed.
    pub completed_at: u64,
}

impl core::fmt::Debug for SessionRecord {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SessionRecord")
            .field("shared_secret", &"<redacted>")
            .field("completed_at", &self.completed_at)
            .finish()
    }
}

/// In-memory map of `PeerId → SessionRecord`. Thread-safe and cheap to
/// clone (Arc-shared inner state). The owner is `NetworkNode`; downstream
/// crates (block production, encrypted gossip) consult it to find a
/// session key for the peer they are about to talk to.
#[derive(Clone, Default)]
pub struct SessionStore {
    inner: Arc<Mutex<HashMap<PeerId, SessionRecord>>>,
}

impl SessionStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed handshake. Overwrites any prior record for the
    /// same peer (the most recent handshake always wins).
    pub fn insert(&self, peer: PeerId, record: SessionRecord) {
        // Mutex poisoning would mean another thread panicked while holding
        // the lock; recovering the inner map is fine because our stored
        // data is plain old types.
        let mut g = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.insert(peer, record);
    }

    /// Look up an existing record. Returns `None` if no handshake has
    /// completed (yet) for the peer.
    pub fn get(&self, peer: &PeerId) -> Option<SessionRecord> {
        let g = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.get(peer).cloned()
    }

    /// Forget a peer's session (called on disconnect or eviction).
    pub fn remove(&self, peer: &PeerId) -> Option<SessionRecord> {
        let mut g = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.remove(peer)
    }

    /// Number of active sessions.
    pub fn len(&self) -> usize {
        let g = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.len()
    }

    /// Whether any sessions are active.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// libp2p request_response Codec
// ---------------------------------------------------------------------------

/// Protocol name newtype used by [`HandshakeCodec`].
#[derive(Clone, Debug, Default)]
pub struct HandshakeProtocol;

/// Bincode codec for [`HandshakeHello`] / [`HandshakeAck`] that plugs into
/// `libp2p::request_response::Behaviour`.
#[derive(Clone, Debug, Default)]
pub struct HandshakeCodec;

#[async_trait]
impl Codec for HandshakeCodec {
    type Protocol = StreamProtocol;
    type Request = HandshakeHello;
    type Response = HandshakeAck;

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let bytes = read_length_prefixed(io, MAX_HANDSHAKE_BYTES).await?;
        bincode::deserialize(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let bytes = read_length_prefixed(io, MAX_HANDSHAKE_BYTES).await?;
        bincode::deserialize(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bytes = bincode::serialize(&req)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        write_length_prefixed(io, &bytes).await
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        resp: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bytes = bincode::serialize(&resp)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        write_length_prefixed(io, &bytes).await
    }
}

/// Build a `StreamProtocol` constant suitable for
/// `request_response::Behaviour::with_codec(HandshakeCodec, [(StreamProtocol::new(HANDSHAKE_PROTOCOL_NAME), ...)])`.
///
/// Returned as a function to keep the `StreamProtocol::new` call site
/// inside this module (the type's constructor is `const` but the
/// borrow checker dislikes leaking `&'static str` references from
/// non-`const` contexts in some libp2p versions).
#[must_use]
pub fn stream_protocol() -> StreamProtocol {
    StreamProtocol::new(HANDSHAKE_PROTOCOL_NAME)
}

/// Type alias for the configured request-response behaviour.
pub type HandshakeBehaviour = libp2p::request_response::Behaviour<HandshakeCodec>;

/// Build a default-configured handshake behaviour. Single supported
/// protocol; inbound and outbound enabled.
#[must_use]
pub fn build_handshake_behaviour() -> HandshakeBehaviour {
    libp2p::request_response::Behaviour::new(
        std::iter::once((stream_protocol(), libp2p::request_response::ProtocolSupport::Full)),
        libp2p::request_response::Config::default(),
    )
}

// ---------------------------------------------------------------------------
// Length-prefixed framing helpers
// ---------------------------------------------------------------------------

async fn read_length_prefixed<T>(io: &mut T, max: usize) -> io::Result<Vec<u8>>
where
    T: AsyncRead + Unpin + Send,
{
    let mut len_buf = [0u8; 4];
    io.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("handshake frame too large: {len} > {max}"),
        ));
    }
    let mut buf = vec![0u8; len];
    io.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn write_length_prefixed<T>(io: &mut T, bytes: &[u8]) -> io::Result<()>
where
    T: AsyncWrite + Unpin + Send,
{
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame >4 GiB"))?;
    io.write_all(&len.to_be_bytes()).await?;
    io.write_all(bytes).await?;
    io.flush().await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use qv_crypto::generate_hybrid_keypair;

    fn fresh_peer() -> PeerId {
        let kp = libp2p::identity::Keypair::generate_ed25519();
        PeerId::from_public_key(&kp.public())
    }

    #[test]
    fn happy_path_roundtrip_derives_matching_secret() {
        let init_pid = fresh_peer();
        let resp_pid = fresh_peer();
        let init_kp = generate_hybrid_keypair(DEFAULT_KEM_LEVEL).unwrap();

        let hello = make_hello(&init_pid, &init_kp);
        let (ack, ss_resp, init_seen) = respond_to_hello(&hello, &resp_pid).unwrap();
        assert_eq!(init_seen, init_pid);

        let (ss_init, resp_seen) = process_ack(&ack, &init_kp, &init_pid).unwrap();
        assert_eq!(resp_seen, resp_pid);

        // Both sides derive the same 32-byte secret.
        assert_eq!(ss_init.as_bytes(), ss_resp.as_bytes());
    }

    #[test]
    fn version_mismatch_rejected() {
        let init_pid = fresh_peer();
        let resp_pid = fresh_peer();
        let init_kp = generate_hybrid_keypair(DEFAULT_KEM_LEVEL).unwrap();
        let mut hello = make_hello(&init_pid, &init_kp);
        hello.version = 999;
        let err = respond_to_hello(&hello, &resp_pid).unwrap_err();
        assert!(matches!(err, NetError::UnsupportedVersion { .. }));
    }

    #[test]
    fn tampered_binding_rejected() {
        let init_pid = fresh_peer();
        let resp_pid = fresh_peer();
        let init_kp = generate_hybrid_keypair(DEFAULT_KEM_LEVEL).unwrap();
        let hello = make_hello(&init_pid, &init_kp);
        let (mut ack, _ss, _) = respond_to_hello(&hello, &resp_pid).unwrap();

        // Flip one bit of the binding.
        ack.session_binding[0] ^= 0x01;
        let err = process_ack(&ack, &init_kp, &init_pid).unwrap_err();
        match err {
            NetError::Codec(msg) => assert!(msg.contains("session binding mismatch")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn wrong_initiator_peer_id_breaks_binding() {
        // If the responder mis-identifies the initiator, the binding will
        // diverge and the initiator must reject the ack. We simulate by
        // swapping the responder's peer id in the ack — the initiator
        // recomputes the binding against the (wrong) responder pid and
        // detects the mismatch.
        let init_pid = fresh_peer();
        let resp_pid = fresh_peer();
        let other_pid = fresh_peer();
        let init_kp = generate_hybrid_keypair(DEFAULT_KEM_LEVEL).unwrap();
        let hello = make_hello(&init_pid, &init_kp);
        let (mut ack, _ss, _) = respond_to_hello(&hello, &resp_pid).unwrap();
        ack.responder_peer_id = other_pid.to_bytes();
        let err = process_ack(&ack, &init_kp, &init_pid).unwrap_err();
        assert!(matches!(err, NetError::Codec(_)));
    }

    #[test]
    fn session_store_basic_lifecycle() {
        let store = SessionStore::new();
        let peer = fresh_peer();
        assert!(store.is_empty());

        // Build a record with a real shared secret so the type signature
        // matches production usage exactly.
        let init_kp = generate_hybrid_keypair(DEFAULT_KEM_LEVEL).unwrap();
        let (_ct, ss) = encapsulate_hybrid(&init_kp.public).unwrap();
        let record = SessionRecord {
            shared_secret: ss.clone(),
            completed_at: 1_700_000_000,
        };
        store.insert(peer, record);
        assert_eq!(store.len(), 1);

        let got = store.get(&peer).unwrap();
        assert_eq!(got.shared_secret.as_bytes(), ss.as_bytes());

        let removed = store.remove(&peer).unwrap();
        assert_eq!(removed.completed_at, 1_700_000_000);
        assert!(store.is_empty());
    }

    #[test]
    fn hello_ack_bincode_roundtrip() {
        let init_pid = fresh_peer();
        let resp_pid = fresh_peer();
        let init_kp = generate_hybrid_keypair(DEFAULT_KEM_LEVEL).unwrap();
        let hello = make_hello(&init_pid, &init_kp);
        let (ack, _ss, _) = respond_to_hello(&hello, &resp_pid).unwrap();

        let hello_bytes = bincode::serialize(&hello).unwrap();
        assert!(hello_bytes.len() < MAX_HANDSHAKE_BYTES);
        let hello_back: HandshakeHello = bincode::deserialize(&hello_bytes).unwrap();
        assert_eq!(hello_back, hello);

        let ack_bytes = bincode::serialize(&ack).unwrap();
        assert!(ack_bytes.len() < MAX_HANDSHAKE_BYTES);
        let ack_back: HandshakeAck = bincode::deserialize(&ack_bytes).unwrap();
        assert_eq!(ack_back, ack);
    }

    #[test]
    fn stream_protocol_constant_matches_module_const() {
        assert_eq!(stream_protocol().as_ref(), HANDSHAKE_PROTOCOL_NAME);
    }
}
