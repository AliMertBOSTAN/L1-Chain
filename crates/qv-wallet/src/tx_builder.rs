//\! Transaction builder.
use crate::{WalletError, WalletResult};
use qv_core::{Transaction, TxInput, TxOutput, ValidityInterval};

#[derive(Clone, Debug)]
pub struct TxBuilder {
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub validity: ValidityInterval,
}

impl TxBuilder {
    pub fn new(validity: ValidityInterval) -> Self {
        TxBuilder {
            inputs: Vec::new(),
            outputs: Vec::new(),
            validity,
        }
    }

    pub fn add_input(&mut self, input: TxInput) -> &mut Self {
        self.inputs.push(input);
        self
    }

    pub fn add_output(&mut self, output: TxOutput) -> &mut Self {
        self.outputs.push(output);
        self
    }

    pub fn build_unsigned(&self) -> WalletResult<Transaction> {
        if self.inputs.is_empty() || self.outputs.is_empty() {
            return Err(WalletError::TxBuilder("missing inputs or outputs".into()));
        }

        Ok(Transaction {
            version: qv_core::TX_VERSION,
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            validity: self.validity.clone(),
            witness: vec\![],
        })
    }

    pub fn serialize(&self) -> WalletResult<Vec<u8>> {
        let tx = self.build_unsigned()?;
        bincode::serialize(&tx).map_err(|e| WalletError::Bincode(e))
    }
}
