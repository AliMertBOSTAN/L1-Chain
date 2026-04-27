# QuantumVault Core UTXO Data Structures

## Overview

This document describes the core data structures for the QuantumVault blockchain project, focusing on UTXO (Unspent Transaction Output) handling. All structures are in the `qv::core` namespace and follow modern C++20 best practices.

## Files Structure

```
include/qv/core/
├── types.hpp           # Fundamental types and constants
├── result.hpp          # Error handling pattern (Result<T, E>)
├── transaction.hpp     # Transaction structures
├── block.hpp           # Block structures
└── utxo.hpp            # UTXO set interface and implementation

src/core/
├── transaction.cpp     # Transaction implementation
├── block.cpp           # Block implementation
└── utxo.cpp            # UTXO set implementation
```

## Core Types (`types.hpp`)

### Basic Types

- **TxId**: 32-byte transaction hash (SHA256)
- **BlockHash**: 32-byte block hash (SHA256)
- **HashDigest**: 32-byte generic hash for merkle roots and commitments
- **Height**: Block height (uint64_t)
- **Amount**: Satoshi-like smallest units (uint64_t)
- **Timestamp**: Unix timestamp in seconds
- **OutputIndex**: Output index within a transaction (uint32_t)
- **bytes**: Generic byte vector for binary data

### OutPoint Structure

```cpp
struct OutPoint {
    TxId tx_id;           // Hash of the transaction
    OutputIndex index;    // Index of the output being spent
};
```

An `OutPoint` uniquely identifies a specific UTXO and is used as the reference when spending outputs.

### Constants

- `MAX_TX_SIZE`: 4 MB per transaction
- `MAX_TX_INPUTS`: 10,000 inputs per transaction
- `MAX_TX_OUTPUTS`: 10,000 outputs per transaction
- `MAX_SUPPLY`: 21 million units (Bitcoin-like cap)
- `ONE_UNIT`: 100 million satoshis

## Error Handling (`result.hpp`)

The codebase uses a Rust-inspired `Result<T, E>` pattern for error handling:

```cpp
Result<Transaction, std::string> create_transaction(...);

auto result = create_transaction(...);
if (result.is_ok()) {
    auto tx = result.value();
    // Use transaction
} else {
    auto error = result.error();
    // Handle error
}
```

### Result Operations

- `is_ok() / is_err()`: Check success/error status
- `value() / error()`: Get the contained value or error (unchecked)
- `ok() / err()`: Get optional<T> or optional<E>
- `unwrap()`: Get value or throw exception
- `unwrap_or(default)`: Get value or return default
- `map() / map_err()`: Transform contained values

## Transaction Structure (`transaction.hpp`)

### TxInput

```cpp
struct TxInput {
    OutPoint prev_output;      // UTXO being spent
    bytes signature;           // Authorization proof
    bytes witness_data;        // Script validation stack
};
```

### TxOutput

```cpp
struct TxOutput {
    Amount value;              // Value in satoshis
    bytes locking_script;      // Spending conditions
    bytes stealth_address_data; // Optional privacy data
};
```

### Transaction

```cpp
class Transaction {
    uint32_t version;                    // Protocol version
    std::vector<TxInput> inputs;         // Inputs (UTXOs to spend)
    std::vector<TxOutput> outputs;       // Outputs (new UTXOs)
    uint64_t lock_time;                  // Block height/timestamp lock
};
```

### Key Methods

- `is_valid()`: Validates transaction structure
- `compute_txid()`: Generates SHA256 double hash ID
- `to_bytes() / from_bytes()`: Serialization
- `total_output_value()`: Sum of all outputs
- `is_coinbase()`: Check if creates new coins
- `verify_locktime()`: Validate temporal constraints

## Block Structure (`block.hpp`)

### BlockHeader

```cpp
class BlockHeader {
    uint32_t version;                    // Protocol version
    BlockHash prev_hash;                 // Link to previous block
    HashDigest merkle_root;              // Merkle tree root of transactions
    HashDigest utxo_commitment;          // UTXO set state commitment
    Timestamp timestamp;                 // Block creation time
    uint64_t nonce;                      // Proof-of-work nonce
    uint64_t difficulty_target;          // PoW difficulty threshold
    bytes pos_proof;                     // Proof-of-stake data
};
```

### Block

```cpp
class Block {
    BlockHeader header;                  // Block metadata
    std::vector<Transaction> transactions; // Transactions in block
};
```

### Key Methods

- `is_valid()`: Validate block structure
- `compute_hash()`: Generate block hash
- `compute_merkle_root()`: Calculate merkle tree root
- `compute_utxo_commitment()`: Hash resulting UTXO state
- `verify_merkle_root()`: Check merkle root against transactions
- `merkle_root_from_hashes()`: Static helper for merkle computation

## UTXO Set (`utxo.hpp`)

### UTXOEntry

```cpp
struct UTXOEntry {
    TxOutput output;        // The output data
    Height block_height;    // Block height when created
    bool is_coinbase;       // Whether from coinbase transaction
};
```

Tracks whether a UTXO is spendable based on coinbase maturity period.

### IUTXOSet Interface

Abstract interface for UTXO operations:

```cpp
class IUTXOSet {
    // Core operations
    Result<void> add(OutPoint, UTXOEntry);
    Result<UTXOEntry> spend(OutPoint);
    optional<UTXOEntry> get(OutPoint) const;
    bool contains(OutPoint) const;

    // State queries
    size_t size() const;
    bool is_empty() const;
    Amount total_value() const;

    // Commitment for consensus
    Result<HashDigest> compute_commitment() const;

    void clear();
};
```

### InMemoryUTXOSet Implementation

Complete in-memory hash map implementation suitable for full nodes:

```cpp
class InMemoryUTXOSet : public IUTXOSet {
    // Implements all IUTXOSet methods
    
    // Additional batch operations
    Result<void> apply_batch(
        const std::vector<std::pair<OutPoint, UTXOEntry>>& to_add,
        const std::vector<OutPoint>& to_spend
    );
    
    std::vector<OutPoint> all_outpoints() const;
    std::vector<UTXOEntry> all_entries() const;
};
```

### UTXO Set Semantics

1. **Adding UTXOs**: Each transaction output creates a new UTXO identified by (txid, output_index)
2. **Spending UTXOs**: Transactions consume UTXOs via inputs referencing OutPoints
3. **Commitment**: A hash digest of all unspent outputs for light client verification
4. **Maturity**: Coinbase outputs require a maturity period (default: 100 blocks) before spending

## Implementation Notes

### Serialization

All structures support `to_bytes()` and `from_bytes()` methods:

```cpp
Result<bytes, std::string> tx = transaction.to_bytes();
Result<Transaction, std::string> restored = Transaction::from_bytes(tx_bytes);
```

Current serialization format (little-endian, varint for sizes):
- Simple binary format with explicit length fields
- Not yet encrypted or checksummed
- TODO: Implement full deserialization (stubs return "not implemented" errors)

### Cryptography

The codebase includes stubs for cryptographic operations:
- SHA256 hashing stubs in `compute_txid()` and `compute_hash()`
- Signature verification stubs in transaction inputs
- Merkle tree uses XOR for placeholder purposes

**TODO**: Replace with actual crypto library:
- OpenSSL or libsodium for hashing
- ECDSA or Ed25519 for signatures
- liboqs for post-quantum cryptography

### Concurrency

Current implementation is single-threaded. For production:
- Use `std::shared_mutex` for reader-writer locks on UTXO set
- Implement copy-on-write semantics for block application
- Consider lock-free data structures for high-throughput scenarios

## Building

```bash
mkdir build
cd build
cmake ..
make
```

Requires C++20 compiler (GCC 10+, Clang 13+, MSVC 2019+).

Optional dependencies for tests:
- Google Test (GTest)
- Google Benchmark

## Usage Example

```cpp
#include "qv/core/transaction.hpp"
#include "qv/core/block.hpp"
#include "qv/core/utxo.hpp"

using namespace qv::core;

// Create a transaction
Transaction tx;
tx.version = 1;
tx.inputs.push_back(TxInput{
    OutPoint{txid, 0},
    bytes{/* signature */},
    bytes{/* witness */}
});
tx.outputs.push_back(TxOutput{
    1'000'000,  // 1 unit = 100M satoshis
    bytes{/* script */}
});

// Compute transaction ID
auto txid = tx.compute_txid();
if (txid.is_ok()) {
    std::cout << "Transaction created successfully\n";
}

// Create UTXO set
auto utxo_set = create_utxo_set(true);

// Add unspent outputs
UTXOEntry entry{tx.outputs[0], 100, false};
auto result = utxo_set->add(OutPoint{txid.value(), 0}, entry);
if (result.is_ok()) {
    std::cout << "UTXO added\n";
}
```

## Future Enhancements

1. **Database Backend**: RocksDB or LevelDB for persistent UTXO storage
2. **Merkle Proof Trees**: For efficient light client proofs
3. **State Channels**: For off-chain transactions
4. **Privacy Features**: Stealth addresses, ring signatures, zero-knowledge proofs
5. **Performance**: SIMD optimizations, batch processing
6. **Consensus**: Implement proof-of-work and proof-of-stake validation
