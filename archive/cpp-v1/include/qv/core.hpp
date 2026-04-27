#pragma once

/// QuantumVault Core UTXO Library
/// Master include file for all core components

#include "qv/core/types.hpp"
#include "qv/core/result.hpp"
#include "qv/core/transaction.hpp"
#include "qv/core/block.hpp"
#include "qv/core/utxo.hpp"

namespace qv {

/// Core namespace containing blockchain primitives
///
/// This namespace provides:
/// - Fundamental types (TxId, BlockHash, Amount, etc.)
/// - Error handling via Result pattern
/// - Transaction structures and validation
/// - Block structures with merkle tree computation
/// - UTXO set management and commitment hashing
///
/// Typical usage:
/// ```cpp
/// using namespace qv::core;
///
/// auto tx = create_transaction(...);
/// auto block = create_block(std::move(tx), ...);
/// auto utxo_set = create_utxo_set(true);
/// ```

using core::TxId;
using core::BlockHash;
using core::Height;
using core::Amount;
using core::Timestamp;
using core::OutPoint;
using core::bytes;

using core::TxInput;
using core::TxOutput;
using core::Transaction;

using core::BlockHeader;
using core::Block;

using core::UTXOEntry;
using core::IUTXOSet;
using core::InMemoryUTXOSet;
using core::create_utxo_set;

using core::Result;

} // namespace qv
