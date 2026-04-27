//\! Stealth scanner.
use crate::{WalletError, WalletResult};
use qv_core::{Amount, OutPoint};
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
