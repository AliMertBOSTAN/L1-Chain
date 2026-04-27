//\! HD derivation (placeholder).
use crate::{WalletError, WalletResult};
use qv_privacy::StealthKeys;

pub trait SeedDeriver: Send + Sync {
    fn derive_account(&self, seed: &[u8; 64], account_idx: u32) -> WalletResult<StealthKeys>;
}

pub struct DefaultSeedDeriver;

impl SeedDeriver for DefaultSeedDeriver {
    fn derive_account(&self, _seed: &[u8; 64], _account_idx: u32) -> WalletResult<StealthKeys> {
        Err(WalletError::HdDerivation("not implemented".into()))
    }
}
