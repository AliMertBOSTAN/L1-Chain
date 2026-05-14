//! Coin selection.
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
        CoinSelector {
            utxos,
            fee_per_byte,
        }
    }

    pub fn select(&self, target: Amount) -> WalletResult<CoinSelection> {
        if self.utxos.is_empty() {
            return Err(WalletError::CoinSelection("no utxos".into()));
        }
        let mut selected = Vec::new();
        let mut total = Amount::ZERO;
        // Reserve a flat 1000-unit buffer for fees / change-dust avoidance.
        const RESERVE: u64 = 1000;
        for (op, amt) in &self.utxos {
            selected.push(*op);
            total = total
                .checked_add(*amt)
                .ok_or_else(|| WalletError::CoinSelection("amount overflow".into()))?;
            let needed = target
                .as_u64()
                .checked_add(RESERVE)
                .ok_or_else(|| WalletError::CoinSelection("target overflow".into()))?;
            if total.as_u64() >= needed {
                let change = total.checked_sub(target);
                return Ok(CoinSelection {
                    selected,
                    total,
                    change,
                });
            }
        }
        Err(WalletError::CoinSelection("insufficient".into()))
    }
}
