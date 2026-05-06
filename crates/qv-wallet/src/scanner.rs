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

            // Reconstruct the StealthOutput from the transaction output's stealth_info.
            let stealth_output = qv_privacy::stealth::StealthOutput {
                kem_ciphertext: stealth_info.ephemeral_pubkey.clone(),
                kyber_level: 3, // Default to Level3; could be derived from stealth_info if encoded
                view_tag: stealth_info.view_tag,
                onetime_pk_hash: [0u8; 32], // Placeholder; will be checked in scan_output
            };

            // Attempt to scan the output.
            match qv_privacy::stealth::scan_output(stealth_keys, &stealth_output) {
                Ok(Some(_scan_result)) => {
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
