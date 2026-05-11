# C-06 spike — ml-dsa 0.0.4 verification

Throwaway scratch project. Goal: prove that `ml-dsa = "0.0.4"` (RustCrypto) supports
deterministic seeded keygen via `MlDsa65::from_seed(&B32)` before we commit to swapping
`qv-crypto::pqc_sign::from_seed` over.

## Run

```sh
cd spikes/c06-mldsa
cargo run --release
```

Expected output: 6 ✅ checkmarks ending with "all 6 checks passed."

## Decision rule

- **All 6 ✅ pass** → green light to land C-06 swap. Drop `pqcrypto-dilithium`'s
  `from_seed_pqc` Err stub and delegate to `ml-dsa`. Closes C-04 + C-06.
- **Any compile error or assert failure** → write down the error, ping the parent
  conversation, do NOT touch `crates/qv-crypto`. We pivot to alternative crates
  (e.g. `pqcrypto-mldsa`, `oqs-rs`).

## Notes

- The parent workspace `Cargo.toml` excludes `spikes/`, so this builds standalone
  with its own `Cargo.lock`. It does **not** affect the main workspace.
- After spike concludes (whether success or failure), this directory can be deleted
  or kept as a reference. It is `publish = false` and has no internal coupling.
