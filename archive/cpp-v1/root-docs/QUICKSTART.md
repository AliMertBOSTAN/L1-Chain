# QuantumVault Core - Quick Start Guide

## Building the Project

```bash
cd "C:\Users\mbostan\Desktop\L1\L1 Blockchain"
mkdir build
cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
cmake --build . --config Release
```

## Basic Usage

### Including the Library

```cpp
#include "qv/core.hpp"

using namespace qv::core;
```

### Creating a Transaction

```cpp
// Create inputs
TxId prev_txid = {...};  // 32-byte hash
TxInput input1{
    OutPoint{prev_txid, 0},           // Spending output 0 from prev_txid
    bytes{0x48, 0x47, ...},           // Signature bytes
    bytes{0x20, ...}                   // Witness data
};

// Create outputs
TxOutput output1{
    1'000'000,                         // 1 unit = 1M satoshis
    bytes{0x76, 0xa9, ...},           // Locking script (OP_DUP OP_HASH160...)
    bytes{}                            // No stealth data
};

// Assemble transaction
Transaction tx;
tx.version = 1;
tx.inputs.push_back(input1);
tx.outputs.push_back(output1);
tx.lock_time = 0;  // No lock

// Validate
if (!tx.is_valid()) {
    std::cerr << "Transaction is invalid\n";
    return;
}

// Compute transaction ID
auto txid_result = tx.compute_txid();
if (!txid_result.is_ok()) {
    std::cerr << "Error: " << txid_result.error() << "\n";
    return;
}
TxId txid = txid_result.value();

// Serialize for transmission
auto bytes_result = tx.to_bytes();
if (bytes_result.is_ok()) {
    const auto& tx_bytes = bytes_result.value();
    std::cout << "Serialized transaction size: " << tx_bytes.size() << " bytes\n";
}
```

### Creating a Block

```cpp
// Create block header
BlockHeader header;
header.version = 1;
header.prev_hash = parent_block_hash;  // 32-byte hash of parent
header.timestamp = std::time(nullptr); // Current time
header.nonce = 12345;                  // PoW nonce
header.difficulty_target = 0x00000000ffff0000;

// Add transactions (first should be coinbase)
std::vector<Transaction> transactions;
transactions.push_back(coinbase_tx);
transactions.push_back(user_tx1);
transactions.push_back(user_tx2);

// Create block
Block block{header, transactions};

// Compute and verify merkle root
auto merkle_result = block.compute_merkle_root();
if (merkle_result.is_ok()) {
    block.header.merkle_root = merkle_result.value();
}

// Compute UTXO commitment
auto commitment_result = block.compute_utxo_commitment();
if (commitment_result.is_ok()) {
    block.header.utxo_commitment = commitment_result.value();
}

// Validate block structure
if (!block.is_valid()) {
    std::cerr << "Block validation failed\n";
    return;
}

// Compute block hash
auto hash_result = block.compute_hash();
if (hash_result.is_ok()) {
    BlockHash block_hash = hash_result.value();
    std::cout << "Block hash computed\n";
}
```

### Managing UTXO Set

```cpp
// Create in-memory UTXO set
auto utxo_set = create_utxo_set(true);

// Add a UTXO after transaction confirmation
OutPoint outpoint{txid, 0};  // First output of our transaction
UTXOEntry entry{
    tx.outputs[0],           // The output
    block_height,            // Height at which it was created
    false                     // Not from coinbase
};
auto add_result = utxo_set->add(outpoint, entry);
if (!add_result.is_ok()) {
    std::cerr << "Error adding UTXO: " << add_result.error() << "\n";
}

// Check if UTXO exists
if (utxo_set->contains(outpoint)) {
    std::cout << "UTXO is in set\n";
}

// Get UTXO details
auto utxo = utxo_set->get(outpoint);
if (utxo.has_value()) {
    std::cout << "UTXO value: " << utxo->output.value << " satoshis\n";
    std::cout << "Created at block: " << utxo->block_height << "\n";
}

// Spend a UTXO
auto spend_result = utxo_set->spend(outpoint);
if (spend_result.is_ok()) {
    std::cout << "UTXO spent successfully\n";
} else {
    std::cerr << "Error spending UTXO: " << spend_result.error() << "\n";
}

// Query set state
std::cout << "Total UTXOs: " << utxo_set->size() << "\n";
std::cout << "Total value: " << utxo_set->total_value() << " satoshis\n";

// Compute commitment for consensus
auto commitment = utxo_set->compute_commitment();
if (commitment.is_ok()) {
    // Use for light client verification, chain validation, etc.
}

// Batch operations (apply entire block)
std::vector<std::pair<OutPoint, UTXOEntry>> new_utxos;
std::vector<OutPoint> spent_utxos;

// Populate from block transactions...
for (const auto& block_tx : block.transactions) {
    // Add new outputs
    for (std::size_t i = 0; i < block_tx.outputs.size(); ++i) {
        new_utxos.emplace_back(
            OutPoint{computed_txid, static_cast<uint32_t>(i)},
            UTXOEntry{block_tx.outputs[i], current_height, block_tx.is_coinbase()}
        );
    }
    
    // Mark inputs as spent
    for (const auto& input : block_tx.inputs) {
        spent_utxos.push_back(input.prev_output);
    }
}

// Apply all changes atomically
auto batch_result = utxo_set->apply_batch(new_utxos, spent_utxos);
if (!batch_result.is_ok()) {
    std::cerr << "Error applying block: " << batch_result.error() << "\n";
    // On error, UTXO set state unchanged
}
```

## Error Handling Examples

### Using Result Pattern

```cpp
// Method 1: Check is_ok()
auto result = transaction.compute_txid();
if (result.is_ok()) {
    TxId txid = result.value();
    // Use txid
} else {
    std::cout << "Error: " << result.error() << "\n";
}

// Method 2: Use optional
auto txid_opt = transaction.compute_txid().ok();
if (txid_opt.has_value()) {
    TxId txid = txid_opt.value();
    // Use txid
}

// Method 3: Unwrap with default
auto utxo_opt = utxo_set->get(outpoint);
Amount value = utxo_opt.value_or(UTXOEntry{}).output.value;

// Method 4: Chain operations with map
auto result = transaction.compute_txid()
    .map([](const TxId& id) { return id.size(); });
```

## Data Structure Overview

### Key Types

```cpp
TxId txid;                          // 32-byte transaction hash
BlockHash block_hash;               // 32-byte block hash
OutPoint outpoint{txid, 0};         // Reference to UTXO (txid + index)
Amount amount = 1'000'000;          // Satoshi-like units
Height height = 500'000;            // Block height (uint64_t)
Timestamp timestamp = time(nullptr); // Unix seconds
```

### Structures

```cpp
TxInput input{outpoint, sig_bytes, witness_bytes};
TxOutput output{amount, script_bytes, stealth_bytes};
Transaction tx{version, {input}, {output}, lock_time};

BlockHeader header{...};
Block block{header, {tx1, tx2, ...}};

UTXOEntry entry{output, height, is_coinbase};
```

## Common Patterns

### Creating a Coinbase Transaction

```cpp
Transaction coinbase;
coinbase.version = 1;
coinbase.lock_time = 0;

// Coinbase input (null outpoint + coinbase data)
TxId null_id{};  // All zeros
TxInput cb_input{
    OutPoint{null_id, 0xFFFFFFFF},
    bytes{},
    bytes{} // Can contain block height, miner info, etc.
};
coinbase.inputs.push_back(cb_input);

// Miner reward output
TxOutput reward{
    6'250'000'000,  // 6.25 units = 625M satoshis (Bitcoin-like halving)
    bytes{...}      // Mining pool address
};
coinbase.outputs.push_back(reward);
```

### Checking Coinbase Maturity

```cpp
if (utxo->is_coinbase) {
    constexpr Height MATURITY = 100;
    Height age = current_block_height - utxo->block_height;
    if (age < MATURITY) {
        std::cout << "Coinbase output must mature for " 
                  << (MATURITY - age) << " more blocks\n";
    }
}
```

### Validating Transaction Inputs

```cpp
for (const auto& input : tx.inputs) {
    // Check UTXO exists
    auto utxo = utxo_set->get(input.prev_output);
    if (!utxo.has_value()) {
        std::cout << "UTXO not found (double-spend or invalid)\n";
        return false;
    }
    
    // Check it's not a coinbase or has matured
    if (utxo->is_coinbase) {
        if (current_height - utxo->block_height < 100) {
            std::cout << "Coinbase output not yet mature\n";
            return false;
        }
    }
    
    // TODO: Verify signature matches locking script
    // TODO: Check witness data satisfies script conditions
}
```

## Performance Tips

1. **Batch Operations**: Use `apply_batch()` instead of individual add/spend calls
2. **Snapshots**: Call `all_outpoints()` once if iterating multiple times
3. **Avoid Copies**: Use `const` references and move semantics
4. **Serialization Caching**: Cache serialized forms for frequently-broadcast items

## Debugging

### Enable Logging

```cpp
// Add to your code
auto bytes_result = tx.to_bytes();
if (bytes_result.is_err()) {
    const auto& error_msg = bytes_result.error();
    std::cerr << "Serialization failed: " << error_msg << "\n";
}
```

### Inspecting UTXO Set

```cpp
// Get all outpoints
auto outpoints = utxo_set->all_outpoints();
std::cout << "Total UTXOs: " << outpoints.size() << "\n";

// Get all values
auto entries = utxo_set->all_entries();
std::uint64_t total = 0;
for (const auto& entry : entries) {
    total += entry.output.value;
}
std::cout << "Total value: " << total << " satoshis\n";
```

## Next Steps

1. Replace crypto stubs with OpenSSL/libsodium
2. Implement transaction script validation
3. Add mempool (transaction pool) management
4. Integrate with P2P networking
5. Implement consensus rules (PoW/PoS)
6. Add block validation and chain synchronization

See `CORE_STRUCTURES.md` for detailed API documentation.
