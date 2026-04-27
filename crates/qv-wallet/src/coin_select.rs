//\! Coin selection.
use crate::{WalletError, WalletResult};
use qv_core::{Amount, OutPoint};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct CoinSelection {
    pub selected: Vec<OutPoint>,
    pub total: Amount,
    pub change: Option<Amount>,
}

pub struct CoinSelector {
    pub utxos: BTreeMap<OutPoint, Amount>,
    pub fee_per_byte: u64,
}

impl CoinSelector {
    pub fn new(utxos: BTreeMap<OutPoint, Amount>, fee_per_byte: u64) -> Self {
        CoinSelector { utxos, fee_per_byte }
    }

    pub fn select(&self, target: Amount) -> WalletResult<CoinSelection> {
        if self.utxos.is_empty() {
            return Err(WalletError::CoinSelection("no utxos".into()));
        }
        let mut selected = Vec::new();
        let mut total = Amount::from_sats(0);
        for (op, amt) in &self.utxos {
            selected.push(op.clone());
            total = total.saturating_add(*amt);
            if total.as_sats() >= target.as_sats() + 1000 {
                return Ok(CoinSelection {
                    selected,
                    total,
                    change: Some(total.saturating_sub(target)),
                });
            }
        }
        Err(WalletError::CoinSelection("insufficient".into()))
    }
}
