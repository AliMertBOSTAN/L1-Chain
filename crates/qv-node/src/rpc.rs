//! JSON-RPC server for QuantumVault queries and transactions.
//!
//! Exposes methods via HTTP and WebSocket subscriptions via `jsonrpsee`.

use std::str::FromStr;
use std::sync::Arc;

use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use qv_consensus::epoch::EpochNonce;
use qv_consensus::slot::SlotClock;
use qv_consensus::stake::StakeDistribution;
use qv_consensus::ChainState;
use qv_core::{Amount, Block, BlockHash, Height, OutPoint, Transaction, TxId};
use qv_crypto::{DilithiumLevel, HybridKeyPair, KyberLevel, PqcPublicKey};
use qv_privacy::stealth::{scan_output_view, StealthOutput as PrivacyStealthOutput};
use qv_mempool::clear::{ClearPool, MempoolEntry};
use qv_mempool::encrypted::EncryptedPool;
use qv_storage::block_store::BlockStore;
use qv_storage::kv::KvStore;
use qv_storage::utxo_store::UtxoStore;
use tokio::sync::mpsc;

use crate::node::NodeEvent;

/// Describes the RPC API surface.
#[rpc(server, client)]
pub trait QvNodeApi {
    /// Get a block by its hash.
    #[method(name = "qv_getBlockByHash")]
    async fn get_block_by_hash(&self, block_hash: String) -> RpcResult<Option<Block>>;

    /// Get a block by its height (indexed).
    #[method(name = "qv_getBlockByHeight")]
    async fn get_block_by_height(&self, height: u64) -> RpcResult<Option<Block>>;

    /// Get the current tip (latest block header).
    #[method(name = "qv_getTip")]
    async fn get_tip(&self) -> RpcResult<TipInfo>;

    /// Get a transaction by ID.
    #[method(name = "qv_getTx")]
    async fn get_tx(&self, tx_id: String) -> RpcResult<Option<Transaction>>;

    /// Submit a signed transaction to the mempool.
    #[method(name = "qv_sendTransaction")]
    async fn send_transaction(&self, tx_bytes: String) -> RpcResult<TxId>;

    /// Get a UTXO by outpoint.
    #[method(name = "qv_getUtxo")]
    async fn get_utxo(&self, outpoint: String) -> RpcResult<Option<UtxoInfo>>;

    /// Scan the current UTXO set for plain `p2pkh_pqc(pubkey_hash)` outputs.
    ///
    /// Used by wallets to discover "non-stealth" funds — most importantly,
    /// genesis allocations and outputs received from senders that did not
    /// produce a stealth-locked payment. Returns every matching outpoint
    /// with its value. Spending uses the regular Dilithium spend key with
    /// `sign_with` / `sign_inputs` (plain p2pkh, no `shared_secret`).
    #[method(name = "qv_scanP2pkh")]
    async fn scan_p2pkh(&self, pubkey_hash_hex: String) -> RpcResult<Vec<P2pkhMatch>>;

    /// Sum the value of every stealth UTXO that the given view key can
    /// decapsulate (ADR-011 Faz 4). The spend secret never leaves the
    /// client — only the view key and the spend **public** key are sent.
    /// The `view_key` payload is the structured [`StealthViewKey`] form.
    #[method(name = "qv_getBalanceFor")]
    async fn get_balance_for(&self, view_key: StealthViewKey) -> RpcResult<u64>;

    /// Scan the current UTXO set for outputs that the given view key can
    /// detect (ADR-011 Faz 4). Returns every matching outpoint together
    /// with the `shared_secret` and `onetime_pk_hash` needed to spend it.
    ///
    /// `from_height` / `to_height` are presently best-effort — the UTXO
    /// set is not height-indexed, so the entire live set is scanned and
    /// the range parameters are ignored. They are preserved on the wire so
    /// a future height-indexed scan can adopt them without a breaking change.
    #[method(name = "qv_scanStealth")]
    async fn scan_stealth(
        &self,
        view_key: StealthViewKey,
        from_height: u64,
        to_height: u64,
    ) -> RpcResult<Vec<StealthScan>>;

    /// Get mempool status.
    #[method(name = "qv_getMempoolStatus")]
    async fn get_mempool_status(&self) -> RpcResult<MempoolStatus>;

    /// Get the currently active stake distribution.
    ///
    /// This is the per-epoch frozen snapshot of `(pool_id, stake)` pairs
    /// used by VRF leader election. Stake pool operators (`qv-miner`)
    /// query this endpoint at startup and at every epoch boundary.
    #[method(name = "qv_getStakeDistribution")]
    async fn get_stake_distribution(&self) -> RpcResult<StakeDistributionSnapshot>;

    /// Get the current epoch nonce.
    ///
    /// The nonce is a 32-byte seed that parameterises VRF leader election
    /// for the current epoch. It evolves at every epoch boundary as
    /// `η_e = SHA3-256(η_{e-1} || extra_entropy || boundary_block_hash)`.
    /// Stake pool operators (`qv-miner`) must use the latest nonce when
    /// evaluating leadership.
    #[method(name = "qv_getEpochNonce")]
    async fn get_epoch_nonce(&self) -> RpcResult<EpochNonceInfo>;

    /// Drain the clear mempool snapshot and return every pending
    /// transaction in deterministic (fee-density descending, then
    /// tx-id ascending) order.
    ///
    /// Each entry is hex-encoded bincode bytes — clients deserialize
    /// to `Transaction`. The mempool itself is *not* mutated by this
    /// call; it is a snapshot read. Used by `qv-miner` to fill block
    /// bodies after winning a slot.
    #[method(name = "qv_getPendingTransactions")]
    async fn get_pending_transactions(&self) -> RpcResult<Vec<String>>;

    /// Submit a fully-signed block to the node.
    ///
    /// The payload is hex-encoded bincode bytes of `qv_core::Block`.
    /// The node performs structural validation, chain-linkage check,
    /// applies the block to UTXO storage, and gossips it to peers.
    /// Returns the canonical hex of the accepted block hash.
    #[method(name = "qv_submitBlock")]
    async fn submit_block(&self, block_bytes: String) -> RpcResult<String>;

    // Subscription endpoints are deferred until the node wires up a real
    // event source (block/tx notifier channels). Re-add when implementing:
    //
    //   #[subscription(name = "qv_subscribeNewBlocks",
    //                  unsubscribe = "qv_unsubscribeNewBlocks", item = Block)]
    //   async fn subscribe_new_blocks(&self) -> jsonrpsee::core::SubscriptionResult;
    //
    //   #[subscription(name = "qv_subscribeNewTx",
    //                  unsubscribe = "qv_unsubscribeNewTx", item = Transaction)]
    //   async fn subscribe_new_tx(&self) -> jsonrpsee::core::SubscriptionResult;
}

/// Information about the chain tip.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct TipInfo {
    pub block_hash: String,
    pub height: u64,
    pub timestamp: u64,
}

/// Information about a UTXO.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct UtxoInfo {
    pub value: u64,
    pub script_hash: String,
    pub has_datum: bool,
    pub has_stealth: bool,
}

/// Stealth UTXO match returned by `qv_scanStealth` (ADR-011 Faz 4).
///
/// `shared_secret_hex` and `onetime_pk_hash_hex` are what the wallet needs
/// to construct a spend witness (`<sig> <spend_pk> <shared_secret>` against
/// the output's `stealth_p2pkh(onetime_pk_hash)` locking script — see ADR-011
/// and ADR-012). They are sensitive: holding them, together with the spend
/// secret key, lets the holder spend this UTXO. The client that submitted
/// the view key is presumed to be the legitimate owner.
///
/// `height` is unused for now (see [`QvNodeApi::scan_stealth`] docs); the
/// field is preserved on the wire for a future height-indexed scan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct StealthScan {
    /// Reserved — currently always `0` (UTXO set is not height-indexed).
    pub height: u64,
    /// Hex-encoded `TxId` of the funding transaction.
    pub tx_id: String,
    /// Output index inside the funding transaction.
    pub output_index: u32,
    /// Value of the output in smallest units.
    pub value: u64,
    /// Recovered shared secret (32-byte hex). Needed to spend the UTXO.
    pub shared_secret_hex: String,
    /// One-time public-key hash committed to by the output's locking
    /// script (32-byte hex).
    pub onetime_pk_hash_hex: String,
}

/// One match returned by `qv_scanP2pkh` — a plain `p2pkh_pqc` UTXO
/// locked to the queried public-key hash.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct P2pkhMatch {
    /// Hex-encoded funding `TxId`.
    pub tx_id: String,
    /// Output index inside the funding transaction.
    pub output_index: u32,
    /// Value (smallest units).
    pub value: u64,
}

/// Wire payload that carries a stealth view-key over JSON-RPC.
///
/// Bundles the recipient's hybrid view keypair (Kyber + X25519, including
/// the secrets) and their **spend public key**. The spend secret never
/// appears here — it is not needed for scanning, only for spending, and
/// the wallet keeps it locally.
///
/// Hex strings are lower-case and unprefixed. Lengths are validated by
/// [`Self::into_view_keys`] against the declared `kyber_level`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct StealthViewKey {
    /// Kyber parameter set: 1, 3, or 5.
    pub kyber_level: u8,
    /// Dilithium parameter set used by the spend key: 2, 3, or 5.
    pub dilithium_level: u8,
    /// X25519 public key (32 bytes), hex.
    pub x25519_pk_hex: String,
    /// X25519 secret key (32 bytes), hex.
    pub x25519_sk_hex: String,
    /// Kyber public key, hex.
    pub kyber_pk_hex: String,
    /// Kyber secret key, hex.
    pub kyber_sk_hex: String,
    /// Dilithium spend public key, hex.
    pub spend_pk_hex: String,
}

impl StealthViewKey {
    /// Parse the wire form into a usable hybrid keypair + spend public key.
    ///
    /// Validates every byte-length and rejects unknown parameter levels.
    /// On success the returned `HybridKeyPair` carries the secret material
    /// in zeroize-on-drop buffers — the server should drop it as soon as
    /// the scan finishes.
    pub fn into_view_keys(&self) -> Result<(HybridKeyPair, PqcPublicKey), String> {
        let kyber_level = match self.kyber_level {
            1 => KyberLevel::Level1,
            3 => KyberLevel::Level3,
            5 => KyberLevel::Level5,
            other => return Err(format!("unknown Kyber level: {other}")),
        };
        let dilithium_level = match self.dilithium_level {
            2 => DilithiumLevel::Level2,
            3 => DilithiumLevel::Level3,
            5 => DilithiumLevel::Level5,
            other => return Err(format!("unknown Dilithium level: {other}")),
        };

        let x25519_pk_bytes =
            hex::decode(&self.x25519_pk_hex).map_err(|e| format!("x25519_pk_hex: {e}"))?;
        let x25519_pk: [u8; 32] = x25519_pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "x25519_pk_hex must decode to exactly 32 bytes".to_string())?;
        let x25519_sk =
            hex::decode(&self.x25519_sk_hex).map_err(|e| format!("x25519_sk_hex: {e}"))?;
        let kyber_pk =
            hex::decode(&self.kyber_pk_hex).map_err(|e| format!("kyber_pk_hex: {e}"))?;
        let kyber_sk =
            hex::decode(&self.kyber_sk_hex).map_err(|e| format!("kyber_sk_hex: {e}"))?;

        let view_kp =
            HybridKeyPair::from_raw_parts(kyber_level, x25519_pk, x25519_sk, kyber_pk, kyber_sk)
                .map_err(|e| format!("view keypair: {e}"))?;

        let spend_pk_bytes =
            hex::decode(&self.spend_pk_hex).map_err(|e| format!("spend_pk_hex: {e}"))?;
        let spend_pk = PqcPublicKey::from_bytes(dilithium_level, &spend_pk_bytes)
            .map_err(|e| format!("spend_pk: {e}"))?;

        Ok((view_kp, spend_pk))
    }
}

/// Current mempool status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct MempoolStatus {
    pub clear_pool_size: usize,
    pub encrypted_pool_size: usize,
    pub min_fee_rate: u64,
    pub total_value: u64,
}

/// One row in a stake distribution snapshot: `(pool_id_hex, stake)`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct PoolStakeInfo {
    /// SHA3-256 of the operator's VRF public key, lower-case hex.
    pub pool_id: String,
    /// Pool's absolute stake in smallest units.
    pub stake: u64,
}

/// Per-epoch frozen stake distribution snapshot returned by
/// `qv_getStakeDistribution`. Pools are sorted by `pool_id` for
/// deterministic wire serialisation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct StakeDistributionSnapshot {
    /// Epoch this snapshot applies to.
    pub epoch: u64,
    /// Sum of all pool stakes.
    pub total_stake: u64,
    /// Per-pool stake entries, sorted by `pool_id`.
    pub pools: Vec<PoolStakeInfo>,
}

impl StakeDistributionSnapshot {
    /// Build a snapshot from a `StakeDistribution`.
    #[must_use]
    pub fn from_distribution(d: &StakeDistribution) -> Self {
        let pools: Vec<PoolStakeInfo> = d
            .iter()
            .map(|(pid, stake)| PoolStakeInfo {
                pool_id: pid.0.to_hex(),
                stake: *stake,
            })
            .collect();
        Self {
            epoch: d.epoch.as_u64(),
            total_stake: d.total_stake(),
            pools,
        }
    }
}

/// Current epoch nonce returned by `qv_getEpochNonce`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct EpochNonceInfo {
    /// 32-byte nonce, lower-case hex.
    pub nonce_hex: String,
    /// Epoch this nonce parameterises (the *current* epoch).
    pub epoch: u64,
}

impl EpochNonceInfo {
    /// Build the wire representation from a typed `EpochNonce` and epoch number.
    #[must_use]
    pub fn from_nonce(nonce: &EpochNonce, epoch: qv_core::Epoch) -> Self {
        Self {
            nonce_hex: hex::encode(nonce.as_bytes()),
            epoch: epoch.as_u64(),
        }
    }
}

/// RPC server holding references to storage, mempool, and consensus layers.
///
/// Note: `chain_state` and `clear_pool` are Mutex-wrapped because the Node
/// main loop mutates them. RPC queries acquire read locks briefly.
///
/// `stake_distribution` and `epoch_nonce` are `RwLock`-wrapped because the
/// epoch boundary handler updates them in-place at the start of every epoch
/// while RPC callers (`qv-miner`, observability tooling) only read them.
pub struct RpcServer<S: KvStore> {
    block_store: Arc<BlockStore<S>>,
    utxo_store: Arc<UtxoStore<S>>,
    chain_state: Arc<tokio::sync::Mutex<ChainState>>,
    clear_pool: Arc<tokio::sync::Mutex<ClearPool>>,
    encrypted_pool: Arc<tokio::sync::Mutex<EncryptedPool>>,
    stake_distribution: Arc<tokio::sync::RwLock<StakeDistribution>>,
    epoch_nonce: Arc<tokio::sync::RwLock<EpochNonce>>,
    /// Slot/epoch math derived from the node's `ProtocolParams`. Cheap to
    /// clone (just consensus constants). Used to map the tip slot to the
    /// current epoch for `qv_getEpochNonce`.
    slot_clock: SlotClock,
    /// Sender for dispatching events back into the node's main loop.
    /// Used by `qv_submitBlock` to hand the validated block to the same
    /// pipeline that processes network-sourced blocks (linkage check,
    /// UTXO apply, gossip relay).
    event_tx: mpsc::Sender<NodeEvent>,
}

impl<S: KvStore> RpcServer<S> {
    /// Create a new RPC server with references to the storage, consensus, and mempool layers.
    #[allow(clippy::too_many_arguments)] // ledger-facing endpoint surface; one ref per concern.
    pub fn new(
        block_store: Arc<BlockStore<S>>,
        utxo_store: Arc<UtxoStore<S>>,
        chain_state: Arc<tokio::sync::Mutex<ChainState>>,
        clear_pool: Arc<tokio::sync::Mutex<ClearPool>>,
        encrypted_pool: Arc<tokio::sync::Mutex<EncryptedPool>>,
        stake_distribution: Arc<tokio::sync::RwLock<StakeDistribution>>,
        epoch_nonce: Arc<tokio::sync::RwLock<EpochNonce>>,
        slot_clock: SlotClock,
        event_tx: mpsc::Sender<NodeEvent>,
    ) -> Self {
        Self {
            block_store,
            utxo_store,
            chain_state,
            clear_pool,
            encrypted_pool,
            stake_distribution,
            epoch_nonce,
            slot_clock,
            event_tx,
        }
    }
}

#[async_trait::async_trait]
impl<S: KvStore + Send + Sync + 'static> QvNodeApiServer for RpcServer<S> {
    async fn get_block_by_hash(&self, block_hash: String) -> RpcResult<Option<Block>> {
        let hash = BlockHash::from_hex(&block_hash).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid block hash: {}", e),
                None::<()>,
            )
        })?;

        tracing::debug!(hash = %hash, "RPC: getBlockByHash");

        match self.block_store.get_block(&hash).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("storage error: {}", e),
                None::<()>,
            )
        })? {
            Some(block) => Ok(Some(block)),
            None => Ok(None),
        }
    }

    async fn get_block_by_height(&self, height: u64) -> RpcResult<Option<Block>> {
        let h = Height::from(height);
        tracing::debug!(height = %height, "RPC: getBlockByHeight");

        match self.block_store.get_block_by_height(h).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("storage error: {}", e),
                None::<()>,
            )
        })? {
            Some(block) => Ok(Some(block)),
            None => Ok(None),
        }
    }

    async fn get_tip(&self) -> RpcResult<TipInfo> {
        let chain = self.chain_state.lock().await;
        let tip = chain.tip();
        tracing::debug!("RPC: getTip");

        // Use canonical lower-case hex via `Display` (not the truncated
        // `Debug` form). Clients parse this back with `BlockHash::from_hex`.
        Ok(TipInfo {
            block_hash: tip.hash.to_hex(),
            height: tip.height.as_u64(),
            timestamp: 0, // ChainEntry doesn't store timestamp
        })
    }

    async fn get_tx(&self, tx_id: String) -> RpcResult<Option<Transaction>> {
        let target_id = TxId::from_hex(&tx_id).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid tx id: {}", e),
                None::<()>,
            )
        })?;

        tracing::debug!(tx_id = %tx_id, "RPC: getTx");

        // Search mempool first
        {
            let pool = self.clear_pool.lock().await;
            for entry in pool.all_sorted() {
                if entry.tx_id == target_id {
                    return Ok(Some(entry.tx.clone()));
                }
            }
        }

        // Then search recent blocks (iterate from tip backwards up to k blocks)
        let ancestors: Vec<_> = {
            let chain = self.chain_state.lock().await;
            let tip_hash = chain.tip().hash;
            chain.ancestors(tip_hash, 50).into_iter().cloned().collect()
        }; // lock released here

        for entry in &ancestors {
            if let Ok(Some(block)) = self.block_store.get_block(&entry.hash) {
                for tx in &block.transactions {
                    if tx.id().ok() == Some(target_id) {
                        return Ok(Some(tx.clone()));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn send_transaction(&self, tx_bytes: String) -> RpcResult<TxId> {
        // Hex-decode the transaction bytes
        let raw_bytes = hex::decode(&tx_bytes).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid hex encoding: {}", e),
                None::<()>,
            )
        })?;

        // Deserialize as bincode
        let tx: Transaction = bincode::deserialize(&raw_bytes).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid transaction encoding: {}", e),
                None::<()>,
            )
        })?;

        // Validate transaction structure
        tx.validate_structure().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("transaction validation failed: {}", e),
                None::<()>,
            )
        })?;

        // Compute transaction ID
        let tx_id = tx.id().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("failed to compute tx id: {}", e),
                None::<()>,
            )
        })?;

        // Simplified fee: use 0 for now; full validation uses qv-node/validation.rs
        // which resolves UTXOs and computes real fee. RPC insertion is a fast path.
        let fee = Amount::from_smallest_units(0u64);
        let estimated_size = bincode::serialized_size(&tx).unwrap_or(0) as usize;

        // Create mempool entry and insert into clear mempool via Mutex
        {
            let entry = MempoolEntry::new(tx, tx_id, fee, estimated_size);
            let mut pool = self.clear_pool.lock().await;
            pool.add(entry).map_err(|e| {
                jsonrpsee::types::ErrorObject::owned(
                    -32603,
                    format!("mempool insertion failed: {}", e),
                    None::<()>,
                )
            })?;
        }

        tracing::info!(%tx_id, "transaction accepted into mempool via RPC");
        Ok(tx_id)
    }

    async fn get_utxo(&self, outpoint: String) -> RpcResult<Option<UtxoInfo>> {
        let op = OutPoint::from_str(&outpoint).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid outpoint: {}", e),
                None::<()>,
            )
        })?;

        tracing::debug!(outpoint = %op, "RPC: getUtxo");

        match self.utxo_store.get(&op).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("storage error: {}", e),
                None::<()>,
            )
        })? {
            Some(output) => Ok(Some(UtxoInfo {
                value: output.value.as_u64(),
                script_hash: output.locking_script.hash().to_hex(),
                has_datum: output.datum.is_some(),
                has_stealth: output.stealth_info.is_some(),
            })),
            None => Ok(None),
        }
    }

    async fn scan_p2pkh(&self, pubkey_hash_hex: String) -> RpcResult<Vec<P2pkhMatch>> {
        tracing::debug!(%pubkey_hash_hex, "RPC: scanP2pkh");

        let hash_bytes = hex::decode(&pubkey_hash_hex).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("pubkey_hash_hex: {e}"),
                None::<()>,
            )
        })?;
        let pk_hash: [u8; 32] = hash_bytes.as_slice().try_into().map_err(|_| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                "pubkey_hash must decode to exactly 32 bytes",
                None::<()>,
            )
        })?;

        // The canonical p2pkh_pqc locking script for this hash; we compare
        // every UTXO's locking_script bytes against this exact target.
        let expected_script = qv_script::p2pkh_pqc(&pk_hash);

        let entries = self.utxo_store.entries().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("utxo store error: {e}"),
                None::<()>,
            )
        })?;

        let mut matches: Vec<P2pkhMatch> = Vec::new();
        for (outpoint, output) in entries {
            // Stealth outputs and p2pkh outputs are disjoint locking-script
            // shapes; this comparison naturally skips stealth UTXOs.
            if output.locking_script.as_bytes() != expected_script.as_slice() {
                continue;
            }
            matches.push(P2pkhMatch {
                tx_id: outpoint.tx_id.to_hex(),
                output_index: outpoint.index,
                value: output.value.as_u64(),
            });
        }
        matches.sort_by(|a, b| {
            a.tx_id
                .cmp(&b.tx_id)
                .then(a.output_index.cmp(&b.output_index))
        });
        Ok(matches)
    }

    async fn get_balance_for(&self, view_key: StealthViewKey) -> RpcResult<u64> {
        tracing::debug!(
            kyber_level = view_key.kyber_level,
            dilithium_level = view_key.dilithium_level,
            "RPC: getBalanceFor"
        );
        // Reuse `scan_stealth`'s detection logic so the two RPCs cannot
        // disagree about what "ours" means.
        let matches = self
            .scan_stealth(view_key, 0, u64::MAX)
            .await?;
        Ok(matches.iter().map(|m| m.value).sum())
    }

    async fn scan_stealth(
        &self,
        view_key: StealthViewKey,
        from_height: u64,
        to_height: u64,
    ) -> RpcResult<Vec<StealthScan>> {
        tracing::debug!(
            from_height,
            to_height,
            kyber_level = view_key.kyber_level,
            "RPC: scanStealth"
        );

        let (view_kp, spend_pk) = view_key.into_view_keys().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(-32602, format!("invalid view key: {e}"), None::<()>)
        })?;

        let entries = self.utxo_store.entries().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("utxo store error: {e}"),
                None::<()>,
            )
        })?;

        let mut matches: Vec<StealthScan> = Vec::new();
        for (outpoint, output) in entries {
            // Only outputs that carry a stealth payload can possibly be ours.
            let Some(info) = output.stealth_info.as_ref() else {
                continue;
            };

            // Rebuild the qv-privacy view of the output. `onetime_pk_hash` is
            // not carried on-chain (ADR-011); the scanner recomputes it from
            // the recovered shared secret + our spend pk. We cross-check it
            // against the output's locking script below to defeat the 1/256
            // view-tag false positive.
            let probe = PrivacyStealthOutput {
                kem_ciphertext: info.ephemeral_pubkey.clone(),
                kyber_level: info.kyber_level,
                view_tag: info.view_tag,
                onetime_pk_hash: [0u8; 32], // unused by scan_output_view
            };

            let scan = match scan_output_view(&view_kp, &spend_pk, &probe) {
                Ok(Some(r)) => r,
                Ok(None) => continue,
                Err(e) => {
                    // Malformed stealth payload — skip the output, don't abort
                    // the whole scan.
                    tracing::debug!(?outpoint, "skipping malformed stealth output: {e}");
                    continue;
                }
            };

            // Verify the locking script commits to the scanned one-time hash.
            let expected_script = qv_script::stealth_p2pkh(&scan.onetime_pk_hash);
            if output.locking_script.as_bytes() != expected_script.as_slice() {
                // View-tag false positive (1/256 expected). Not actually ours.
                continue;
            }

            matches.push(StealthScan {
                height: 0, // UTXO set is not height-indexed (ADR-011 future work).
                tx_id: outpoint.tx_id.to_hex(),
                output_index: outpoint.index,
                value: output.value.as_u64(),
                shared_secret_hex: hex::encode(scan.shared_secret.as_bytes()),
                onetime_pk_hash_hex: hex::encode(scan.onetime_pk_hash),
            });
        }

        // Deterministic order so repeated calls produce identical wire bytes.
        matches.sort_by(|a, b| {
            a.tx_id
                .cmp(&b.tx_id)
                .then(a.output_index.cmp(&b.output_index))
        });
        Ok(matches)
    }

    async fn get_stake_distribution(&self) -> RpcResult<StakeDistributionSnapshot> {
        tracing::debug!("RPC: getStakeDistribution");
        let dist = self.stake_distribution.read().await;
        Ok(StakeDistributionSnapshot::from_distribution(&dist))
    }

    async fn get_epoch_nonce(&self) -> RpcResult<EpochNonceInfo> {
        tracing::debug!("RPC: getEpochNonce");
        // Snapshot the current nonce and the tip slot; release locks
        // before the slot→epoch translation so we don't hold them across
        // any potentially blocking work.
        let nonce = *self.epoch_nonce.read().await;
        let tip_slot = {
            let chain = self.chain_state.lock().await;
            chain.tip().slot
        };
        let epoch_num = self.slot_clock.slot_to_epoch(tip_slot);
        Ok(EpochNonceInfo::from_nonce(&nonce, epoch_num))
    }

    async fn get_pending_transactions(&self) -> RpcResult<Vec<String>> {
        tracing::debug!("RPC: getPendingTransactions");
        let pool = self.clear_pool.lock().await;
        let entries = pool.all_sorted();
        let mut out: Vec<String> = Vec::with_capacity(entries.len());
        for entry in entries {
            let bytes = bincode::serialize(&entry.tx).map_err(|e| {
                jsonrpsee::types::ErrorObject::owned(
                    -32603,
                    format!("serialize tx {} failed: {e}", entry.tx_id),
                    None::<()>,
                )
            })?;
            out.push(hex::encode(bytes));
        }
        Ok(out)
    }

    async fn submit_block(&self, block_bytes: String) -> RpcResult<String> {
        // 1. Hex decode.
        let raw = hex::decode(&block_bytes).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid hex encoding: {e}"),
                None::<()>,
            )
        })?;

        // 2. Bincode deserialize.
        let block: Block = bincode::deserialize(&raw).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid block encoding: {e}"),
                None::<()>,
            )
        })?;

        // 3. Structural validation — reject malformed blocks at the RPC
        //    boundary so bad input never enters the pipeline.
        block.validate_structure().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("structural validation failed: {e}"),
                None::<()>,
            )
        })?;

        // 4. Compute the block hash for the response *before* dispatch so
        //    callers always get the canonical hash even if the chain-
        //    linkage check later rejects the block (in which case the hash
        //    can be used to look up failure logs).
        let block_hash = block.hash().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("failed to compute block hash: {e}"),
                None::<()>,
            )
        })?;

        let height = block.header.height;
        let tx_count = block.transactions.len();

        // 5. Hand the block to the main loop. Linkage check, UTXO apply,
        //    chain-state update, mempool eviction, and gossip relay all
        //    happen there (`Node::handle_block`).
        self.event_tx
            .send(NodeEvent::BlockReceived(block))
            .await
            .map_err(|e| {
                jsonrpsee::types::ErrorObject::owned(
                    -32603,
                    format!("event channel closed: {e}"),
                    None::<()>,
                )
            })?;

        tracing::info!(
            block_hash = %block_hash,
            height = ?height,
            tx_count,
            "block accepted via RPC; dispatched to node pipeline"
        );

        Ok(block_hash.to_hex())
    }

    async fn get_mempool_status(&self) -> RpcResult<MempoolStatus> {
        let pool = self.clear_pool.lock().await;
        let clear_size = pool.len();

        let enc_pool = self.encrypted_pool.lock().await;
        let encrypted_size = enc_pool.len();
        drop(enc_pool);

        // Compute total value in clear pool (sum of output values)
        let mut total_value = 0u64;
        for entry in pool.all_sorted() {
            // Sum output values as a proxy for pool value
            for output in &entry.tx.outputs {
                total_value = total_value.saturating_add(output.value.as_u64());
            }
        }
        drop(pool);

        Ok(MempoolStatus {
            clear_pool_size: clear_size,
            encrypted_pool_size: encrypted_size,
            min_fee_rate: 1,
            total_value,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_types_serde() {
        let tip = TipInfo {
            block_hash: "abc123".to_string(),
            height: 100,
            timestamp: 1000000,
        };
        let json = serde_json::to_string(&tip).unwrap();
        let _deserialized: TipInfo = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn test_mempool_status_serde() {
        let status = MempoolStatus {
            clear_pool_size: 50,
            encrypted_pool_size: 10,
            min_fee_rate: 1,
            total_value: 1000000,
        };
        let json = serde_json::to_string(&status).unwrap();
        let _deserialized: MempoolStatus = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn test_utxo_info_serde() {
        let utxo = UtxoInfo {
            value: 100000,
            script_hash: "abc".to_string(),
            has_datum: true,
            has_stealth: false,
        };
        let json = serde_json::to_string(&utxo).unwrap();
        let _deserialized: UtxoInfo = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn stealth_scan_serde_roundtrip() {
        let scan = StealthScan {
            height: 0,
            tx_id: "ab".repeat(32),
            output_index: 7,
            value: 50_000,
            shared_secret_hex: "cd".repeat(32),
            onetime_pk_hash_hex: "ef".repeat(32),
        };
        let json = serde_json::to_string(&scan).unwrap();
        let back: StealthScan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tx_id, scan.tx_id);
        assert_eq!(back.output_index, scan.output_index);
        assert_eq!(back.value, scan.value);
        assert_eq!(back.shared_secret_hex, scan.shared_secret_hex);
        assert_eq!(back.onetime_pk_hash_hex, scan.onetime_pk_hash_hex);
    }

    #[test]
    fn stealth_view_key_roundtrips_through_into_view_keys() {
        use qv_privacy::stealth::{create_stealth_output, scan_output_view, StealthKeys};

        // Alice generates a fresh stealth identity and exports the view-key
        // portion to the wire form (`StealthViewKey`). The server re-imports
        // it via `into_view_keys` and must still be able to detect a payment
        // that was created against Alice's published address.
        let alice = StealthKeys::generate(KyberLevel::Level3, DilithiumLevel::Level3).unwrap();
        let wire = StealthViewKey {
            kyber_level: 3,
            dilithium_level: 3,
            x25519_pk_hex: hex::encode(alice.view_kp.public.x25519),
            x25519_sk_hex: hex::encode(alice.view_kp.x25519_secret_bytes()),
            kyber_pk_hex: hex::encode(&alice.view_kp.public.kyber),
            kyber_sk_hex: hex::encode(alice.view_kp.kyber_secret_bytes()),
            spend_pk_hex: hex::encode(alice.spend_kp.public.as_bytes()),
        };

        // Survives a JSON roundtrip on the wire.
        let json = serde_json::to_string(&wire).unwrap();
        let parsed: StealthViewKey = serde_json::from_str(&json).unwrap();
        let (view_kp, spend_pk) = parsed.into_view_keys().expect("parse view key");

        // The reconstructed pair must detect a payment to Alice's address.
        let (stealth_output, _ss) = create_stealth_output(&alice.address()).unwrap();
        let scan = scan_output_view(&view_kp, &spend_pk, &stealth_output)
            .unwrap()
            .expect("reconstructed view key must detect the output");
        assert_eq!(scan.onetime_pk_hash, stealth_output.onetime_pk_hash);
    }

    #[test]
    fn stealth_view_key_rejects_wrong_kyber_level() {
        let alice = qv_privacy::stealth::StealthKeys::generate(
            KyberLevel::Level3,
            DilithiumLevel::Level3,
        )
        .unwrap();
        let wire = StealthViewKey {
            kyber_level: 9, // invalid
            dilithium_level: 3,
            x25519_pk_hex: hex::encode(alice.view_kp.public.x25519),
            x25519_sk_hex: hex::encode(alice.view_kp.x25519_secret_bytes()),
            kyber_pk_hex: hex::encode(&alice.view_kp.public.kyber),
            kyber_sk_hex: hex::encode(alice.view_kp.kyber_secret_bytes()),
            spend_pk_hex: hex::encode(alice.spend_kp.public.as_bytes()),
        };
        let err = wire.into_view_keys().unwrap_err();
        assert!(err.contains("Kyber"));
    }

    #[test]
    fn stake_distribution_snapshot_serde() {
        let snap = StakeDistributionSnapshot {
            epoch: 7,
            total_stake: 1_000_000,
            pools: vec![PoolStakeInfo {
                pool_id: "aa".repeat(32),
                stake: 1_000_000,
            }],
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: StakeDistributionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.epoch, 7);
        assert_eq!(back.pools.len(), 1);
    }

    #[test]
    fn epoch_nonce_info_serde() {
        let info = EpochNonceInfo {
            nonce_hex: "ff".repeat(32),
            epoch: 12,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: EpochNonceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.epoch, 12);
        assert_eq!(back.nonce_hex.len(), 64);
    }
}
