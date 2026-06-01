//! Transaction builder with Dilithium signing.
//!
//! Supports plain `p2pkh_pqc` outputs as well as **stealth outputs**
//! (ADR-011): the sender calls [`TxBuilder::add_stealth_output`] with the
//! recipient's [`StealthAddress`], which performs Kyber KEM encapsulation,
//! attaches a `StealthInfo` payload, and locks the output with
//! `stealth_p2pkh(onetime_pk_hash)`. The recipient — after detecting the
//! output with their view key via [`scan_output`](qv_privacy::stealth::scan_output)
//! — calls [`TxBuilder::sign_stealth_input`] with their spend keypair and the
//! recovered `shared_secret` to produce the witness
//! `<signature> <spend_pk> <shared_secret>`.
use crate::{WalletError, WalletResult};
use qv_core::{Amount, Script as CoreScript, StealthInfo, Transaction, TxInput, TxOutput, ValidityInterval, Witness};
use qv_crypto::{PqcPublicKey, PqcSecretKey, SharedSecret};
use qv_privacy::stealth::{create_stealth_output, StealthAddress};
use qv_script::{stealth_p2pkh, ScriptBuilder};

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
        tx.validity_interval = self.validity;
        Ok(tx)
    }

    pub fn serialize(&self) -> WalletResult<Vec<u8>> {
        let tx = self.build_unsigned()?;
        bincode::serialize(&tx).map_err(WalletError::Bincode)
    }

    /// Sign the transaction using a single secret key and its corresponding public key.
    ///
    /// Signs the transaction's **sighash** — the witness-excluded canonical
    /// hash (ADR-012) — with the given Dilithium secret key. The witness is
    /// encoded as script bytecode that pushes only `signature, public key`
    /// (in that order); the `p2pkh_pqc` locking script derives the signed
    /// message itself via the `SIG_HASH` opcode. The message is no longer
    /// carried in the witness, which closes the in-flight replay vector.
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
        // ADR-012: sign the witness-excluded sighash, not the full canonical
        // bytes. This binds the signature to the transaction without the
        // circular witness dependency.
        let sighash = tx
            .sighash()
            .map_err(|e| WalletError::TxBuilder(format!("sighash failed: {e}")))?;

        let signature = qv_crypto::sign_pqc(secret_key, &sighash)
            .map_err(|e| WalletError::Crypto(e.to_string()))?;

        // Build the witness as script bytecode: push sig, pubkey
        let witness_script = ScriptBuilder::new()
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
    /// The number of keypairs must match the number of inputs. Each input is
    /// signed with its corresponding keypair over the transaction's sighash
    /// (ADR-012), and the witness is encoded as script bytecode that pushes
    /// `signature, public key`. All inputs share one transaction-wide
    /// sighash, so the signing payload is computed once.
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
        // ADR-012: one transaction-wide, witness-excluded sighash; every
        // input signature commits to it.
        let sighash = tx
            .sighash()
            .map_err(|e| WalletError::TxBuilder(format!("sighash failed: {e}")))?;

        // Sign with each keypair and construct the witness script.
        for (input, (secret_key, public_key)) in self.inputs.iter_mut().zip(keypairs.iter()) {
            let signature = qv_crypto::sign_pqc(secret_key, &sighash)
                .map_err(|e| WalletError::Crypto(e.to_string()))?;

            // Build the witness as script bytecode: push sig, pubkey
            let witness_script = ScriptBuilder::new()
                .push_bytes(signature.as_bytes())
                .push_bytes(public_key.as_bytes())
                .build();

            input.witness = Witness::new(witness_script);
        }

        Ok(())
    }

    /// Sign a single **plain** `p2pkh_pqc` input at `input_idx`.
    ///
    /// Witness format: `<signature> <pubkey>` — same as [`Self::sign_with`]
    /// but per-input. Useful when a transaction mixes plain and stealth
    /// inputs (see [`Self::sign_stealth_input`]) — the dispatch must be
    /// done by the caller, since the two witness shapes differ.
    pub fn sign_plain_input(
        &mut self,
        input_idx: usize,
        secret_key: &PqcSecretKey,
        public_key: &PqcPublicKey,
    ) -> WalletResult<()> {
        if self.inputs.get(input_idx).is_none() {
            return Err(WalletError::TxBuilder(format!(
                "plain input index {input_idx} out of range (count {})",
                self.inputs.len()
            )));
        }

        let tx = self.build_unsigned()?;
        let sighash = tx
            .sighash()
            .map_err(|e| WalletError::TxBuilder(format!("sighash failed: {e}")))?;

        let signature = qv_crypto::sign_pqc(secret_key, &sighash)
            .map_err(|e| WalletError::Crypto(e.to_string()))?;

        let witness_script = ScriptBuilder::new()
            .push_bytes(signature.as_bytes())
            .push_bytes(public_key.as_bytes())
            .build();

        if let Some(input) = self.inputs.get_mut(input_idx) {
            input.witness = Witness::new(witness_script);
        } else {
            return Err(WalletError::TxBuilder(format!(
                "plain input index {input_idx} disappeared during signing"
            )));
        }
        Ok(())
    }

    /// Add an output that pays the given [`StealthAddress`] (ADR-011).
    ///
    /// Performs Kyber hybrid-KEM encapsulation against the recipient's view
    /// key to derive a one-time `shared_secret`, then attaches a `StealthInfo`
    /// payload and locks the output with `stealth_p2pkh(onetime_pk_hash)`.
    ///
    /// Returns the `shared_secret` produced by encapsulation. The sender
    /// normally does not need it (only the recipient does, via
    /// [`scan_output`](qv_privacy::stealth::scan_output)), but it is returned
    /// so audit/disclosure tooling can reproduce the commitment if needed.
    /// Drop it as soon as it is no longer required — `SharedSecret` zeroizes
    /// on drop.
    pub fn add_stealth_output(
        &mut self,
        value: Amount,
        recipient: &StealthAddress,
    ) -> WalletResult<SharedSecret> {
        let (stealth_output, shared_secret) = create_stealth_output(recipient)
            .map_err(|e| WalletError::Privacy(e.to_string()))?;

        let locking_script_bytes = stealth_p2pkh(&stealth_output.onetime_pk_hash);
        let locking_script = CoreScript::new(locking_script_bytes);

        let stealth_info = StealthInfo {
            ephemeral_pubkey: stealth_output.kem_ciphertext,
            kyber_level: stealth_output.kyber_level,
            view_tag: stealth_output.view_tag,
        };

        let output = TxOutput::new(value, locking_script).with_stealth(stealth_info);
        self.outputs.push(output);
        Ok(shared_secret)
    }

    /// Sign a single **stealth** input (ADR-011).
    ///
    /// The witness pushed onto the input is `<signature> <spend_pk>
    /// <shared_secret>` (bottom → top). The `stealth_p2pkh` locking script
    /// then:
    ///
    /// 1. recomputes `SHA3-256(STEALTH_KDF_TAG || shared_secret || spend_pk)`
    ///    and checks it against its embedded `onetime_pk_hash` commitment;
    /// 2. derives the message via the `SIG_HASH` opcode (ADR-012);
    /// 3. verifies the signature with `CHECKSIG_PQC(spend_pk, sig, sighash)`.
    ///
    /// `input_idx` must reference an input already added with
    /// [`add_input`](Self::add_input). The signature is computed over the
    /// transaction's sighash, which is identical for every input.
    pub fn sign_stealth_input(
        &mut self,
        input_idx: usize,
        spend_secret: &PqcSecretKey,
        spend_public: &PqcPublicKey,
        shared_secret: &SharedSecret,
    ) -> WalletResult<()> {
        if self.inputs.get(input_idx).is_none() {
            return Err(WalletError::TxBuilder(format!(
                "stealth input index {input_idx} out of range (count {})",
                self.inputs.len()
            )));
        }

        let tx = self.build_unsigned()?;
        let sighash = tx
            .sighash()
            .map_err(|e| WalletError::TxBuilder(format!("sighash failed: {e}")))?;

        let signature = qv_crypto::sign_pqc(spend_secret, &sighash)
            .map_err(|e| WalletError::Crypto(e.to_string()))?;

        // Witness bottom → top: <sig> <spend_pk> <shared_secret>
        // (matches `qv_script::stealth_p2pkh`'s stack expectations).
        let witness_script = ScriptBuilder::new()
            .push_bytes(signature.as_bytes())
            .push_bytes(spend_public.as_bytes())
            .push_bytes(shared_secret.as_bytes())
            .build();

        if let Some(input) = self.inputs.get_mut(input_idx) {
            input.witness = Witness::new(witness_script);
        } else {
            return Err(WalletError::TxBuilder(format!(
                "stealth input index {input_idx} disappeared during signing"
            )));
        }
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use qv_core::{OutPoint, Slot, TxId};
    use qv_crypto::{DilithiumLevel, KyberLevel};
    use qv_privacy::stealth::{scan_output, StealthKeys};
    use qv_script::validate_script;

    fn fresh_stealth_keys() -> StealthKeys {
        StealthKeys::generate(KyberLevel::Level3, DilithiumLevel::Level3).expect("stealth keys")
    }

    #[test]
    fn add_stealth_output_produces_consistent_metadata() {
        let recipient = fresh_stealth_keys();
        let mut builder = TxBuilder::new(ValidityInterval::UNBOUNDED);
        builder.add_input(TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0)));

        let shared_secret = builder
            .add_stealth_output(Amount::from(1_000), &recipient.address())
            .expect("stealth output");
        assert_eq!(builder.outputs.len(), 1);

        let out = &builder.outputs[0];
        let info = out
            .stealth_info
            .as_ref()
            .expect("stealth_info must be attached");
        assert_eq!(info.kyber_level, 3, "Kyber Level 3 used by default");
        assert!(!info.ephemeral_pubkey.is_empty());

        // The locking script must commit to the same one-time hash that the
        // recipient will recompute from `shared_secret` + `spend_pk`.
        let expected_hash =
            qv_privacy::stealth::compute_onetime_pk_hash(&shared_secret, &recipient.spend_kp.public);
        let expected_script = stealth_p2pkh(&expected_hash);
        assert_eq!(
            out.locking_script.as_bytes(),
            expected_script.as_slice(),
            "locking script must encode stealth_p2pkh(onetime_pk_hash)"
        );

        // And the recipient's view-tag must match the on-chain tag.
        let expected_tag = qv_privacy::stealth::compute_view_tag(&shared_secret);
        assert_eq!(info.view_tag, expected_tag);
    }

    #[test]
    fn stealth_create_scan_spend_roundtrip() {
        // ----- Setup: Alice owns stealth keys; Bob sends to her. -----
        let alice = fresh_stealth_keys();
        let alice_addr = alice.address();

        // ----- Step 1: Bob builds the funding tx with a stealth output. -----
        let funding_input_op = OutPoint::new(TxId::from_bytes([0xAB; 32]), 0);
        let mut funding = TxBuilder::new(ValidityInterval::UNBOUNDED);
        funding.add_input(TxInput::new(funding_input_op));
        let _sender_ss = funding
            .add_stealth_output(Amount::from(50_000), &alice_addr)
            .expect("stealth output");

        let funding_tx = funding.build_unsigned().expect("funding build");
        let funding_txid = funding_tx.id().expect("funding id");
        let stealth_output = funding_tx.outputs[0].clone();

        // ----- Step 2: Alice scans the funding output. -----
        let info = stealth_output
            .stealth_info
            .as_ref()
            .expect("stealth_info present");
        // Reconstruct the qv-privacy StealthOutput from the on-chain payload.
        // `onetime_pk_hash` is unused by `scan_output` (ADR-011); it lives in
        // the locking script and we cross-check it below.
        let probe = qv_privacy::stealth::StealthOutput {
            kem_ciphertext: info.ephemeral_pubkey.clone(),
            kyber_level: info.kyber_level,
            view_tag: info.view_tag,
            onetime_pk_hash: [0u8; 32],
        };
        let scan = scan_output(&alice, &probe).expect("scan ok").expect("match");
        // The recipient must verify the locking script binds the recovered
        // commitment — the view tag alone is a 1/256 pre-filter.
        let expected_script = stealth_p2pkh(&scan.onetime_pk_hash);
        assert_eq!(
            stealth_output.locking_script.as_bytes(),
            expected_script.as_slice(),
            "locking script must commit to the scanned one-time hash"
        );

        // ----- Step 3: Alice builds a spending tx and signs the stealth input. -----
        let spending_outpoint = OutPoint::new(funding_txid, 0);
        let mut spend = TxBuilder::new(ValidityInterval::UNBOUNDED);
        spend.add_input(TxInput::new(spending_outpoint));
        spend.add_output(TxOutput::new(
            Amount::from(49_000),
            CoreScript::new(vec![0x01]), // OP_1 (anyone-can-spend) — destination is irrelevant for this test
        ));
        spend.sign_stealth_input(
            0,
            &alice.spend_kp.secret,
            &alice.spend_kp.public,
            &scan.shared_secret,
        )
        .expect("sign stealth input");
        let spending_tx = spend.build_unsigned().expect("spend build");

        // ----- Step 4: Run the full script VM on the spend. -----
        // `validate_script` prepends the witness to the locking script and
        // executes both with a real Context (sighash included).
        let witness_bytes = spending_tx.inputs[0].witness.as_bytes();
        let result = validate_script(
            &stealth_output.locking_script,
            witness_bytes,
            &spending_tx,
            &[stealth_output.clone()],
            Slot::from(7),
        )
        .expect("script VM ok");
        assert!(
            result.success,
            "Alice must be able to spend her stealth UTXO"
        );
    }

    #[test]
    fn stealth_witness_rejected_with_wrong_shared_secret() {
        let alice = fresh_stealth_keys();
        let mut funding = TxBuilder::new(ValidityInterval::UNBOUNDED);
        funding.add_input(TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0)));
        funding
            .add_stealth_output(Amount::from(100), &alice.address())
            .expect("stealth output");
        let funding_tx = funding.build_unsigned().expect("funding");
        let stealth_output = funding_tx.outputs[0].clone();

        // Alice tries to spend, but supplies the WRONG shared secret. The
        // signature is still hers, but the on-chain commitment check fails
        // before any signature verification runs.
        let mut spend = TxBuilder::new(ValidityInterval::UNBOUNDED);
        spend.add_input(TxInput::new(OutPoint::new(
            funding_tx.id().expect("id"),
            0,
        )));
        spend.add_output(TxOutput::new(Amount::from(50), CoreScript::new(vec![0x01])));
        let bogus_secret = SharedSecret([0xFFu8; 32]);
        spend.sign_stealth_input(
            0,
            &alice.spend_kp.secret,
            &alice.spend_kp.public,
            &bogus_secret,
        )
        .expect("witness builds (validation runs at script time)");
        let spending_tx = spend.build_unsigned().expect("spend build");

        let witness_bytes = spending_tx.inputs[0].witness.as_bytes();
        let res = validate_script(
            &stealth_output.locking_script,
            witness_bytes,
            &spending_tx,
            &[stealth_output.clone()],
            Slot::from(7),
        );
        // The commitment check is a VERIFY inside stealth_p2pkh — it returns
        // a hard error (ScriptError::VerifyFailed), not just a falsy stack.
        assert!(res.is_err(), "wrong shared_secret must be rejected");
    }

    #[test]
    fn sign_stealth_input_rejects_out_of_range_index() {
        let alice = fresh_stealth_keys();
        let mut spend = TxBuilder::new(ValidityInterval::UNBOUNDED);
        spend.add_input(TxInput::new(OutPoint::new(TxId::from_bytes([2; 32]), 0)));
        spend.add_output(TxOutput::new(Amount::from(1), CoreScript::new(vec![0x01])));

        let ss = SharedSecret([0x11u8; 32]);
        let err = spend
            .sign_stealth_input(5, &alice.spend_kp.secret, &alice.spend_kp.public, &ss)
            .unwrap_err();
        assert!(matches!(err, WalletError::TxBuilder(_)));
    }
}
