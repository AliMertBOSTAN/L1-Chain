# RPC Rewrite Summary

## Overview
Rewrote `crates/qv-node/src/rpc.rs` to replace mock/stub implementations with real queries against storage, consensus, and mempool layers.

## Key Changes

### 1. RpcServer Struct
**Before:** Empty struct with no fields.
```rust
pub struct RpcServer {
    // Will hold references to storage, network, consensus layer.
}
```

**After:** Generic struct holding Arc-wrapped references to all required layers.
```rust
pub struct RpcServer<S: KvStore> {
    block_store: Arc<BlockStore<S>>,
    utxo_store: Arc<UtxoStore<S>>,
    chain_state: Arc<ChainState>,
    clear_pool: Arc<ClearPool>,
    encrypted_pool: Arc<EncryptedPool>,
}
```

### 2. Constructor
Added `RpcServer::new()` taking explicit Arc references to all layers.

### 3. Method Implementations

#### `get_block_by_hash`
- Parses hex string to `BlockHash` with proper error handling
- Queries `block_store.get_block(&hash)` 
- Returns error if not found
- Properly propagates storage errors

#### `get_block_by_height`
- Converts u64 to `Height` 
- Queries `block_store.get_block_by_height(h)`
- Returns error if not found

#### `get_tip`
- Reads current tip from `chain_state.tip()`
- Extracts hash, height, and formats as TipInfo
- Note: timestamp is 0 (ChainEntry doesn't store it; would need block fetch)

#### `get_tx`
- Parses hex TxId
- Searches `clear_pool` first (unconfirmed txs)
- Then searches recent blocks (up to 50 ancestors from tip)
- Iterates block transactions and compares IDs

#### `send_transaction`
- Hex-decodes tx_bytes
- Bincode-deserializes to Transaction
- Validates transaction structure
- Computes TxId
- Creates MempoolEntry with fee=0 (simplified)
- **Note:** Returns error because ClearPool requires mutable access, which RpcServer cannot have via Arc<>. A future version should wrap ClearPool in Arc<Mutex<>> or Arc<RwLock<>>.

#### `get_utxo`
- Parses OutPoint string (format: `txid#index`)
- Queries `utxo_store.get(&op)`
- Returns UtxoInfo with value, script hash, and flags for datum/stealth

#### `get_balance_for`
- **Stubbed:** Returns error with explanation
- Full implementation would require:
  - Parsing view_key_hex into StealthKeys
  - Iterating all UTXO entries
  - Calling `stealth::scan_output()` on outputs with stealth_info
  - Summing matched values

#### `scan_stealth`
- **Stubbed:** Returns error with explanation
- Full implementation would require:
  - Parsing view_key_hex into StealthKeys
  - Fetching blocks in [from_height, to_height]
  - Calling `stealth::scan_output()` on each output
  - Recording matches as StealthScan results

#### `get_mempool_status`
- Queries `clear_pool.len()` and `encrypted_pool.len()`
- Computes total_value by summing all entry fees
- Returns MempoolStatus with both pool sizes and aggregate fee

## Error Handling

All methods use proper `Result` propagation with `?`:
- Invalid input → `InvalidParams` error code
- Storage errors → `InternalError` 
- Not found → `ServerError(1)`
- Detailed error messages explaining what went wrong

## No Unsafe Code
File adheres to `#![forbid(unsafe_code)]` — zero unsafe blocks.

## No Panics
All methods handle errors gracefully:
- No `unwrap()`, `expect()`, `panic!()`
- No indexing without bounds checks
- Safe error propagation

## Tests
Existing tests preserved and still pass:
- `test_rpc_types_serde()` — TipInfo roundtrip
- `test_mempool_status_serde()` — MempoolStatus roundtrip
- `test_utxo_info_serde()` — UtxoInfo roundtrip

## Known Limitations

1. **Mempool Mutation:** `send_transaction` cannot add to mempool because ClearPool is wrapped in `Arc<>` (immutable). Requires Arc<Mutex<ClearPool>> or Arc<RwLock<ClearPool>>.

2. **Timestamp in get_tip:** ChainEntry doesn't store block timestamp. Would need to fetch the actual Block to get it (adds extra I/O).

3. **Stealth Scanning:** `get_balance_for` and `scan_stealth` are stubbed pending proper key type integration with privacy module.

4. **Fee Calculation:** `send_transaction` assumes fee=0 (would need actual UTXO resolution to compute inputs - outputs).

## File Location
`C:\Users\mbostan\Desktop\L1\L1 Blockchain\crates\qv-node\src\rpc.rs`

## Dependencies Used
- `qv_core` — Block, Transaction, BlockHash, Height, OutPoint, TxId, etc.
- `qv_storage` — BlockStore, UtxoStore, KvStore trait
- `qv_consensus` — ChainState
- `qv_mempool` — ClearPool, EncryptedPool
- `qv_privacy` — StealthKeys (for documentation; not yet used in RPC)
- `jsonrpsee` — RpcResult, error codes
- `hex` — hex encoding/decoding
- `bincode` — Transaction serialization
