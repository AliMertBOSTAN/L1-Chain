//! Transaction builder with Dilithium signing.
use crate::{WalletError, WalletResult};
use qv_core::{Transaction, TxInput, TxOutput, ValidityInterval, Witness};
use qv_crypto::{PqcPublicKey, PqcSecretKey};
use qv_script::ScriptBuilder;

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

        // `Transaction::new` initialises `lock_time` and `fee` to defaults;
        // we only need to override the validity interval here. Per-input
        // witnesses are carried inside each `TxInput`, not on `Transaction`.
        let mut tx = Transaction::new(self.inputs.clone(), self.outputs.clone());
        tx.validity_interval = self.validity.clone();
        Ok(tx)
    }

    pub fn serialize(&self) -> WalletResult<Vec<u8>> {
        let tx = self.build_unsigned()?;
        bincode::serialize(&tx).map_err(WalletError::Bincode)
    }

    /// Sign the transaction using a single secret key and its corresponding public key.
    ///
    /// Computes the canonical transaction bytes as the signing payload,
    /// then signs with the given Dilithium secret key. The witness is encoded
    /// as script bytecode that pushes: message, signature, public key (in that order)
    /// so the stack is ready for the p2pkh_pqc locking script.
    ///
    /// For multi-input transactions, use [`sign_inputs`](Self::sign_inputs)
    /// to provide one keypair per input.
    pub fn sign_with(
        &mut self,
        secret_key: &PqcSecretKey,
        public_key: &PqcPublicKey,
    ) -> WalletResult<()> {
        if self.inputs.is_empty() {
            return Err(WalletError::TxBuilder("no inputs to sign".into()));
        }

        let tx = self.build_unsigned()?;
        let payload = tx
            .canonical_bytes()
            .map_err(|e| WalletError::TxBuilder(format!("tx encoding failed: {e}")))?;

        let signature = qv_crypto::sign_pqc(secret_key, &payload)
            .map_err(|e| WalletError::Crypto(e.to_string()))?;

        // Build the witness as script bytecode: push msg, sig, pubkey
        let witness_script = ScriptBuilder::new()
            .push_bytes(&payload)
            .push_bytes(signature.as_bytes())
            .push_bytes(public_key.as_bytes())
            .build();

        let witness = Witness::new(witness_script);

        // Attach the witness to the first input.
        if let Some(input) = self.inputs.get_mut(0) {
            input.witness = witness;
        } else {
            return Err(WalletError::TxBuilder(
                "failed to attach witness to first input".into(),
            ));
        }

        Ok(())
    }

    /// Sign the transaction using a separate keypair for each input.
    ///
    /// The number of keypairs must match the number of inputs.
    /// Each input is signed with its corresponding keypair, and the witness
    /// is encoded as script bytecode (message, signature, public key).
    pub fn sign_inputs(&mut self, keypairs: &[(PqcSecretKey, PqcPublicKey)]) -> WalletResult<()> {
        if keypairs.len() != self.inputs.len() {
            return Err(WalletError::TxBuilder(format!(
                "keypair count ({}) does not match input count ({})",
                keypairs.len(),
                self.inputs.len()
            )));
        }

        if self.inputs.is_empty() {
            return Err(WalletError::TxBuilder("no inputs to sign".into()));
        }

        let tx = self.build_unsigned()?;
        let payload = tx
            .canonical_bytes()
            .map_err(|e| WalletError::TxBuilder(format!("tx encoding failed: {e}")))?;

        // Sign with each keypair and construct the witness script.
        for (input, (secret_key, public_key)) in self.inputs.iter_mut().zip(keypairs.iter()) {
            let signature = qv_crypto::sign_pqc(secret_key, &payload)
                .map_err(|e| WalletError::Crypto(e.to_string()))?;

            // Build the witness as script bytecode: push msg, sig, pubkey
            let witness_script = ScriptBuilder::new()
                .push_bytes(&payload)
                .push_bytes(signature.as_bytes())
                .push_bytes(public_key.as_bytes())
                .build();

            input.witness = Witness::new(witness_script);
        }

        Ok(())
    }
}
