# QuantumVault Core UTXO Implementation - Complete Index

## Project Information

- **Project Name**: QuantumVault
- **Module**: Core UTXO Data Structures
- **Language**: C++20
- **Standard**: #pragma once, Result error pattern
- **Namespace**: `qv::core`
- **Status**: Complete (Basic Implementation with Cryptographic Stubs)

---

## File Manifest

### Headers - Public API (7 files)

| File | Lines | Purpose | Key Classes |
|------|-------|---------|-------------|
| `include/qv/core.hpp` | 65 | Master include | All types (namespace aliases) |
| `include/qv/core/types.hpp` | 280 | Fundamental types | TxId, BlockHash, OutPoint, Amount, Height |
| `include/qv/core/result.hpp` | 210 | Error handling | Result<T, E>, Result<void, E> |
| `include/qv/core/transaction.hpp` | 380 | Transactions | TxInput, TxOutput, Transaction |
| `include/qv/core/block.hpp` | 380 | Blocks | BlockHeader, Block |
| `include/qv/core/utxo.hpp` | 360 | UTXO management | IUTXOSet, InMemoryUTXOSet |
| `include/qv/core/crypto_stub.hpp` | 30 | Crypto placeholder | (TODO: real crypto library) |

**Total Header Lines**: ~1,705

### Implementation - Source Code (3 files)

| File | Lines | Purpose | Key Functions |
|------|-------|---------|----------------|
| `src/core/transaction.cpp` | 350 | Tx implementation | serialize, hash, validate |
| `src/core/block.cpp` | 450 | Block implementation | merkle tree, commitment, hash |
| `src/core/utxo.cpp` | 320 | UTXO set impl | add, spend, batch apply |

**Total Implementation Lines**: ~1,120

### Documentation (4 files)

| File | Lines | Purpose |
|------|-------|---------|
| `CORE_STRUCTURES.md` | 430 | Detailed technical documentation |
| `UTXO_IMPLEMENTATION_SUMMARY.md` | 320 | High-level overview |
| `QUICKSTART.md` | 380 | Getting started guide with examples |
| `IMPLEMENTATION_INDEX.md` | This file | File manifest and reference |

**Total Documentation Lines**: ~1,530

### Configuration (1 file)

| File | Purpose |
|------|---------|
| `CMakeLists.txt` | Build configuration (updated to include core library) |

---

## Component Hierarchy

```
qv::core namespace
├── Fundamental Types (types.hpp)
│   ├── Hash types: TxId, BlockHash, HashDigest
│   ├── Numeric types: Height, Amount, Timestamp, OutputIndex
│   ├── Compound types: OutPoint
│   └── Constants namespace (MAX_SUPPLY, etc.)
│
├── Error Handling (result.hpp)
│   ├── Result<T, E> template
│   ├── Result<void, E> specialization
│   └── Helper methods (is_ok, map, unwrap, etc.)
│
├── Transactions (transaction.hpp + transaction.cpp)
│   ├── TxInput structure
│   │   ├── prev_output: OutPoint
│   │   ├── signature: bytes
│   │   └── witness_data: bytes
│   ├── TxOutput structure
│   │   ├── value: Amount
│   │   ├── locking_script: bytes
│   │   └── stealth_address_data: bytes
│   └── Transaction class
│       ├── Serialization (to_bytes/from_bytes)
│       ├── Hashing (compute_txid)
│       ├── Validation (is_valid)
│       └── Queries (is_coinbase, verify_locktime)
│
├── Blocks (block.hpp + block.cpp)
│   ├── BlockHeader class
│   │   ├── Metadata (version, timestamp)
│   │   ├── Links (prev_hash)
│   │   ├── Commitments (merkle_root, utxo_commitment)
│   │   └── PoW/PoS fields (nonce, difficulty, pos_proof)
│   ├── Block class
│   │   ├── header: BlockHeader
│   │   ├── transactions: vector<Transaction>
│   │   ├── Validation (is_valid, transactions_valid)
│   │   ├── Hashing (compute_hash)
│   │   ├── Merkle (compute_merkle_root, verify_merkle_root)
│   │   └── Commitment (compute_utxo_commitment)
│   └── Merkle tree helper (merkle_root_from_hashes)
│
└── UTXO Set (utxo.hpp + utxo.cpp)
    ├── UTXOEntry struct
    │   ├── output: TxOutput
    │   ├── block_height: Height
    │   ├── is_coinbase: bool
    │   └── is_spendable() method
    ├── IUTXOSet interface
    │   ├── add(OutPoint, UTXOEntry) -> Result<void>
    │   ├── spend(OutPoint) -> Result<UTXOEntry>
    │   ├── get(OutPoint) -> optional<UTXOEntry>
    │   ├── contains(OutPoint) -> bool
    │   ├── size() -> size_t
    │   ├── total_value() -> Amount
    │   ├── compute_commitment() -> Result<HashDigest>
    │   └── clear() -> void
    ├── InMemoryUTXOSet implementation
    │   ├── Backend: unordered_map<OutPoint, UTXOEntry>
    │   ├── Copy/Move semantics
    │   ├── apply_batch() for atomic operations
    │   ├── all_outpoints() snapshot
    │   └── all_entries() snapshot
    └── Factory function (create_utxo_set)
```

---

## Type Reference

### Hash Types (32 bytes)

```cpp
using TxId = std::array<std::uint8_t, 32>;           // Transaction identifier
using BlockHash = std::array<std::uint8_t, 32>;      // Block identifier
using HashDigest = std::array<std::uint8_t, 32>;     // Generic hash
```

### Numeric Types

```cpp
using Height = std::uint64_t;           // Block height
using Amount = std::uint64_t;           // Satoshi units
using Timestamp = std::uint64_t;        // Unix seconds
using OutputIndex = std::uint32_t;      // Output index in tx
using bytes = std::vector<std::uint8_t>; // Binary data
```

### OutPoint (12 bytes)

```cpp
struct OutPoint {
    TxId tx_id;             // 32 bytes: hash of transaction
    OutputIndex index;      // 4 bytes: output position
};
```

---

## Method Reference

### Transaction Methods

| Method | Signature | Returns | Purpose |
|--------|-----------|---------|---------|
| `is_valid()` | `() const noexcept` | `bool` | Validate structure |
| `is_coinbase()` | `() const noexcept` | `bool` | Check if creates coins |
| `compute_txid()` | `() const` | `Result<TxId>` | Generate transaction ID |
| `to_bytes()` | `() const` | `Result<bytes>` | Serialize to wire format |
| `from_bytes()` | `(const bytes&)` static | `Result<Transaction>` | Deserialize |
| `total_output_value()` | `() const noexcept` | `Amount` | Sum all outputs |
| `verify_locktime()` | `(Height, Timestamp) const noexcept` | `bool` | Check time lock |

### Block Methods

| Method | Signature | Returns | Purpose |
|--------|-----------|---------|---------|
| `is_valid()` | `() const noexcept` | `bool` | Validate structure |
| `compute_hash()` | `() const` | `Result<BlockHash>` | Generate block ID |
| `compute_merkle_root()` | `() const` | `Result<HashDigest>` | Calculate merkle root |
| `verify_merkle_root()` | `() const noexcept` | `bool` | Check against stored |
| `compute_utxo_commitment()` | `() const` | `Result<HashDigest>` | Hash UTXO state |
| `to_bytes()` | `() const` | `Result<bytes>` | Full serialization |
| `from_bytes()` | `(const bytes&)` static | `Result<Block>` | Deserialize |
| `merkle_root_from_hashes()` | `(const vector<TxId>&)` static | `Result<HashDigest>` | Merkle helper |
| `total_inputs()` | `() const noexcept` | `size_t` | Sum of inputs |
| `total_outputs()` | `() const noexcept` | `size_t` | Sum of outputs |

### UTXO Set Methods (IUTXOSet Interface)

| Method | Signature | Returns | Purpose |
|--------|-----------|---------|---------|
| `add()` | `(const OutPoint&, const UTXOEntry&)` | `Result<void>` | Insert UTXO |
| `spend()` | `(const OutPoint&)` | `Result<UTXOEntry>` | Remove UTXO |
| `get()` | `(const OutPoint&) const` | `optional<UTXOEntry>` | Read UTXO |
| `contains()` | `(const OutPoint&) const noexcept` | `bool` | Check existence |
| `size()` | `() const noexcept` | `size_t` | UTXO count |
| `is_empty()` | `() const noexcept` | `bool` | Is empty |
| `total_value()` | `() const noexcept` | `Amount` | Sum all values |
| `compute_commitment()` | `() const` | `Result<HashDigest>` | Consensus hash |
| `clear()` | `() noexcept` | `void` | Reset set |

### InMemoryUTXOSet Additional Methods

| Method | Signature | Returns | Purpose |
|--------|-----------|---------|---------|
| `apply_batch()` | `(const vector<pair>, const vector<OutPoint>&)` | `Result<void>` | Atomic add+spend |
| `all_outpoints()` | `() const` | `vector<OutPoint>` | Get snapshot |
| `all_entries()` | `() const` | `vector<UTXOEntry>` | Get snapshot |

### Result Methods

| Method | Template | Returns | Purpose |
|--------|----------|---------|---------|
| `is_ok()` | `()` | `bool` | Is success |
| `is_err()` | `()` | `bool` | Is error |
| `value()` | `()` | `const T&` | Get success value |
| `error()` | `()` | `const E&` | Get error value |
| `ok()` | `()` | `optional<T>` | Get value as optional |
| `err()` | `()` | `optional<E>` | Get error as optional |
| `unwrap()` | `()` | `T` | Get value or throw |
| `unwrap_or(default)` | `(const T&)` | `T` | Get value or default |
| `map(fn)` | `(F&&)` | `Result<U, E>` | Transform value |
| `map_err(fn)` | `(F&&)` | `Result<T, E2>` | Transform error |

---

## Serialization Format

### Transaction Wire Format

```
uint32_t    version              (4 bytes, little-endian)
uint8_t     input_count          (1 byte, varint simplified)
[inputs]
  TxId      prev_tx_id           (32 bytes)
  uint32_t  prev_output_index    (4 bytes, little-endian)
  uint16_t  signature_length     (2 bytes, little-endian)
  bytes     signature            (variable)
  uint16_t  witness_length       (2 bytes, little-endian)
  bytes     witness_data         (variable)
uint8_t     output_count         (1 byte, varint simplified)
[outputs]
  uint64_t  value                (8 bytes, little-endian)
  uint16_t  script_length        (2 bytes, little-endian)
  bytes     locking_script       (variable)
  uint16_t  stealth_length       (2 bytes, little-endian)
  bytes     stealth_data         (variable)
uint64_t    lock_time            (8 bytes, little-endian)
```

### BlockHeader Wire Format

```
uint32_t    version              (4 bytes)
BlockHash   prev_hash            (32 bytes)
HashDigest  merkle_root          (32 bytes)
HashDigest  utxo_commitment      (32 bytes)
uint64_t    timestamp            (8 bytes)
uint64_t    nonce                (8 bytes)
uint64_t    difficulty_target    (8 bytes)
uint16_t    pos_proof_length     (2 bytes)
bytes       pos_proof            (variable)
```

### Block Wire Format

```
[BlockHeader serialized as above]
uint8_t     tx_count             (1 byte, varint simplified)
[transactions as above, repeated]
```

---

## Constants

### Blockchain Parameters (types.hpp::constants)

```cpp
constexpr uint32_t MAX_TX_SIZE = 4'000'000;           // 4 MB
constexpr uint32_t MAX_TX_INPUTS = 10'000;
constexpr uint32_t MAX_TX_OUTPUTS = 10'000;
constexpr Amount SATOSHI = 1;
constexpr Amount ONE_UNIT = 100'000'000;              // 100M satoshis
constexpr Amount MAX_SUPPLY = 21'000'000 * ONE_UNIT;  // 21M units total
```

### Lock Time Semantics

```cpp
lock_time == 0           → No lock, can spend immediately
0 < lock_time < 500M     → Block height lock (block number)
lock_time >= 500M        → Timestamp lock (Unix seconds)
```

### Coinbase Maturity

```
Coinbase outputs require:
- block_height difference >= 100 blocks
- Before maturity, cannot be spent
- Checked via UTXOEntry::is_spendable()
```

---

## Usage Patterns

### Pattern 1: Create and Validate Transaction

```cpp
using namespace qv::core;

Transaction tx;
tx.version = 1;
tx.inputs.push_back(TxInput{outpoint, sig_bytes, witness_bytes});
tx.outputs.push_back(TxOutput{amount, script_bytes});

if (!tx.is_valid()) return error();
auto txid = tx.compute_txid().unwrap();
```

### Pattern 2: Build and Validate Block

```cpp
BlockHeader header;
header.version = 1;
header.prev_hash = parent_hash;
header.timestamp = now();

Block block{header, {coinbase_tx, user_tx}};
block.header.merkle_root = block.compute_merkle_root().unwrap();
block.header.utxo_commitment = block.compute_utxo_commitment().unwrap();

if (!block.is_valid()) return error();
auto block_hash = block.compute_hash().unwrap();
```

### Pattern 3: Manage UTXO Set

```cpp
auto utxo_set = create_utxo_set(true);

// Add UTXO
utxo_set->add(outpoint, UTXOEntry{output, height, false}).unwrap();

// Check and spend
if (auto utxo = utxo_set->get(outpoint)) {
    if (utxo->is_spendable(current_height)) {
        utxo_set->spend(outpoint).unwrap();
    }
}

// Batch apply block
auto commitment = utxo_set->compute_commitment().unwrap();
```

### Pattern 4: Error Handling

```cpp
auto result = transaction.compute_txid();

// Method A: Direct check
if (result.is_ok()) {
    process(result.value());
} else {
    log_error(result.error());
}

// Method B: Optional
if (auto txid = result.ok()) {
    process(txid.value());
}

// Method C: Transform
auto hash_size = result.map([](const TxId& id) { 
    return id.size(); 
});
if (hash_size.is_ok()) {
    std::cout << "Hash size: " << hash_size.value() << "\n";
}
```

---

## File Locations

### Full Paths

```
C:\Users\mbostan\Desktop\L1\L1 Blockchain\
│
├── include\qv\core\
│   ├── types.hpp
│   ├── result.hpp
│   ├── transaction.hpp
│   ├── block.hpp
│   ├── utxo.hpp
│   ├── crypto_stub.hpp
│   └── (core.hpp goes in include\qv\)
│
├── src\core\
│   ├── transaction.cpp
│   ├── block.cpp
│   └── utxo.cpp
│
├── CMakeLists.txt
├── CORE_STRUCTURES.md
├── UTXO_IMPLEMENTATION_SUMMARY.md
├── QUICKSTART.md
├── IMPLEMENTATION_INDEX.md
└── (other project files)
```

---

## Integration Checklist

- [ ] Replace crypto stubs with OpenSSL/libsodium
- [ ] Implement transaction script validation
- [ ] Implement block deserialization
- [ ] Add database backend support
- [ ] Thread-safe UTXO set with reader-writer locks
- [ ] Mempool (transaction pool) management
- [ ] P2P networking integration
- [ ] Consensus rules (PoW/PoS)
- [ ] Chain synchronization logic
- [ ] Transaction fee calculation
- [ ] UTXO index optimization
- [ ] Unit tests (GTest)
- [ ] Benchmark tests (Google Benchmark)
- [ ] Fuzz testing for serialization

---

## Performance Characteristics

### Time Complexity

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| `add(OutPoint, Entry)` | O(1) | Hash map insertion |
| `spend(OutPoint)` | O(1) | Hash map removal |
| `get(OutPoint)` | O(1) | Hash map lookup |
| `compute_commitment()` | O(n) | n = UTXO count, sorts outpoints |
| `apply_batch(k, m)` | O(k + m) | k adds, m spends |
| `compute_merkle_root(n)` | O(n) | n = transaction count |

### Space Complexity

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| `InMemoryUTXOSet(n)` | O(n) | n = UTXO count |
| `Transaction` | O(inputs + outputs) | Dynamic vectors |
| `Block` | O(transactions) | Dynamic vector |
| Result<T> | O(sizeof(T)) | Value or error, not both |

---

## Known Limitations & TODOs

### High Priority

- [ ] Implement SHA256 double hash (uses placeholders currently)
- [ ] Implement transaction deserialization
- [ ] Implement block deserialization
- [ ] Real ECDSA signature implementation

### Medium Priority

- [ ] Database backend for UTXO set
- [ ] Transaction script VM
- [ ] Smart contract support
- [ ] Thread-safe concurrent access

### Low Priority

- [ ] Benchmark suite
- [ ] Compression for serialization
- [ ] Optimization passes
- [ ] Documentation website

---

## Version History

- **v0.1.0** (Initial Release)
  - Core UTXO structures
  - Transaction and block types
  - In-memory UTXO set
  - Serialization framework
  - Placeholder cryptography

---

## License & Attribution

This implementation is part of the QuantumVault blockchain project.

---

## Support & Resources

- See `CORE_STRUCTURES.md` for detailed API documentation
- See `QUICKSTART.md` for usage examples
- See `UTXO_IMPLEMENTATION_SUMMARY.md` for architecture overview
- CMakeLists.txt shows integration with project build system

---

**Last Updated**: 2026-04-10  
**Status**: Complete (Awaiting Cryptography Integration)
