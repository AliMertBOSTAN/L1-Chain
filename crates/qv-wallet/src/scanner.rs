//! Stealth address scanning for detected outputs.
use crate::WalletResult;
use qv_core::{Amount, Block, OutPoint, Transaction};
use qv_privacy::StealthKeys;
use std::collections::BTreeMap;

pub trait MatchStore: Send + Sync {
    fn add_match(&mut self, outpoint: OutPoint, amount: Amount) -> WalletResult<()>;
    fn get_matches(&self) -> WalletResult<BTreeMap<OutPoint, Amount>>;
}

#[derive(Clone, Debug, Default)]
pub struct MemoryMatchStore {
    matches: BTreeMap<OutPoint, Amount>,
}

impl MemoryMatchStore {
    pub fn new() -> Self {
        MemoryMatchStore::default()
    }
}

impl MatchStore for MemoryMatchStore {
    fn add_match(&mut self, outpoint: OutPoint, amount: Amount) -> WalletResult<()> {
        self.matches.insert(outpoint, amount);
        Ok(())
    }

    fn get_matches(&self) -> WalletResult<BTreeMap<OutPoint, Amount>> {
        Ok(self.matches.clone())
    }
}

/// Stealth address scanner for blocks.
pub struct StealthScanner;

impl StealthScanner {
    /// Scan a block for outputs belonging to the given stealth keys.
    ///
    /// Iterates through all transactions in the block, examining each output
    /// for stealth information. Uses the recipient's view key to check if an
    /// output is ours via [`scan_output`](qv_privacy::stealth::scan_output).
    /// Matching outputs are added to the provided store.
    pub fn scan_block(
        block: &Block,
        stealth_keys: &StealthKeys,
        store: &mut dyn MatchStore,
    ) -> WalletResult<()> {
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            Self::scan_transaction(tx, tx_idx, stealth_keys, store)?;
        }
        Ok(())
    }

    /// Scan a single transaction for stealth outputs.
    ///
    /// Examines each output in the transaction. If it has stealth_info,
    /// attempts to decrypt and match it against the view key.
    fn scan_transaction(
        tx: &Transaction,
        tx_idx: usize,
        stealth_keys: &StealthKeys,
        store: &mut dyn MatchStore,
    ) -> WalletResult<()> {
        for (out_idx, output) in tx.outputs.iter().enumerate() {
            // Skip outputs without stealth info.
            let Some(stealth_info) = &output.stealth_info else {
                continue;
            };

            // Reconstruct the StealthOutput from the on-chain stealth_info.
            // `onetime_pk_hash` is not carried here — it is the output's
            // locking-script commitment, verified below (ADR-011).
            let stealth_output = qv_privacy::stealth::StealthOutput {
                kem_ciphertext: stealth_info.ephemeral_pubkey.clone(),
                kyber_level: stealth_info.kyber_level,
                view_tag: stealth_info.view_tag,
                onetime_pk_hash: [0u8; 32], // unused by scan_output (ADR-011)
            };

            // Attempt to scan the output.
            match qv_privacy::stealth::scan_output(stealth_keys, &stealth_output) {
                Ok(Some(scan_result)) => {
                    // View tag matched. Confirm ownership: the output must be
                    // locked to stealth_p2pkh(onetime_pk_hash) (ADR-011).
                    let expected_script = qv_script::stealth_p2pkh(&scan_result.onetime_pk_hash);
                    if output.locking_script.as_bytes() != expected_script.as_slice() {
                        // 1/256 view-tag false positive — not actually ours.
                        continue;
                    }
                    // This output is ours. Add it to the match store.
                    let tx_id = tx
                        .id()
                        .map_err(|e| crate::WalletError::Core(format!("tx id error: {e}")))?;

                    let outpoint = OutPoint::new(tx_id, out_idx as u32);
                    store.add_match(outpoint, output.value)?;
                }
                Ok(None) => {
                    // Not ours; continue.
                }
                Err(e) => {
                    // Log or skip malformed outputs.
                    tracing::debug!(
                        "stealth scan error at tx[{}].out[{}]: {}",
                        tx_idx,
                        out_idx,
                        e
                    );
                }
            }
        }
        Ok(())
    }
}
