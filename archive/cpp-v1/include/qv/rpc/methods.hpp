#pragma once

#include <memory>
#include <nlohmann/json.hpp>
#include "server.hpp"

namespace qv::rpc {

using json = nlohmann::json;

// Forward declarations
class RpcServer;

/**
 * @brief RPC Method implementations
 *
 * Standard methods for blockchain data retrieval and transaction submission.
 * All methods follow JSON-RPC 2.0 specification.
 */
class RpcMethods {
 public:
  /**
   * @brief Register all standard methods with the server
   * @param server The RPC server to register methods with
   */
  static void register_all(RpcServer& server);

  // ============ Block Methods ============

  /**
   * @brief getblock - Get block by hash or height
   *
   * Params:
   *   - hash_or_height: string (hash) or number (height)
   *   - verbose: bool (default: true)
   *
   * Returns:
   *   {
   *     "hash": "0x...",
   *     "height": 12345,
   *     "version": 1,
   *     "previous_hash": "0x...",
   *     "merkle_root": "0x...",
   *     "timestamp": 1234567890,
   *     "difficulty_target": 0x0000ffff,
   *     "nonce": 123456,
   *     "transactions": ["txid1", "txid2", ...],
   *     "size": 512,
   *     "weight": 2048
   *   }
   */
  static std::optional<json> getblock(const json& params);

  /**
   * @brief getblockheader - Get block header by hash
   *
   * Params:
   *   - hash: string (block hash)
   *   - verbose: bool (default: true)
   *
   * Returns:
   *   {
   *     "hash": "0x...",
   *     "version": 1,
   *     "previous_hash": "0x...",
   *     "merkle_root": "0x...",
   *     "timestamp": 1234567890,
   *     "difficulty_target": 0x0000ffff,
   *     "nonce": 123456
   *   }
   */
  static std::optional<json> getblockheader(const json& params);

  /**
   * @brief gettransaction - Get transaction by ID
   *
   * Params:
   *   - txid: string (transaction hash)
   *   - verbose: bool (default: true)
   *
   * Returns:
   *   {
   *     "txid": "0x...",
   *     "version": 1,
   *     "inputs": [
   *       {
   *         "previous_txid": "0x...",
   *         "previous_output_index": 0,
   *         "script": "0x..."
   *       }
   *     ],
   *     "outputs": [
   *       {
   *         "amount": 50000000,
   *         "script": "0x..."
   *       }
   *     ],
   *     "locktime": 0,
   *     "size": 256
   *   }
   */
  static std::optional<json> gettransaction(const json& params);

  // ============ UTXO Methods ============

  /**
   * @brief getutxo - Get unspent output by outpoint
   *
   * Params:
   *   - txid: string (transaction hash)
   *   - vout: number (output index)
   *
   * Returns:
   *   {
   *     "amount": 50000000,
   *     "script": "0x...",
   *     "coinbase": false,
   *     "confirmations": 100
   *   }
   *   or null if spent
   */
  static std::optional<json> getutxo(const json& params);

  /**
   * @brief getbalance - Get balance for stealth address
   *
   * Params:
   *   - stealth_address: string (encoded stealth address)
   *   - min_confirmations: number (default: 1)
   *
   * Returns:
   *   {
   *     "confirmed": 1000000000,
   *     "unconfirmed": 500000000,
   *     "total": 1500000000
   *   }
   */
  static std::optional<json> getbalance(const json& params);

  // ============ Transaction Methods ============

  /**
   * @brief sendrawtransaction - Submit a raw transaction
   *
   * Params:
   *   - tx_hex: string (hex-encoded transaction)
   *   - allow_highfees: bool (default: false)
   *
   * Returns:
   *   {
   *     "txid": "0x...",
   *     "status": "accepted_to_mempool"
   *   }
   */
  static std::optional<json> sendrawtransaction(const json& params);

  /**
   * @brief decoderawtransaction - Decode raw transaction
   *
   * Params:
   *   - tx_hex: string (hex-encoded transaction)
   *
   * Returns: Decoded transaction structure
   */
  static std::optional<json> decoderawtransaction(const json& params);

  // ============ Chain Methods ============

  /**
   * @brief getblockchaininfo - Get blockchain statistics
   *
   * Params: (none)
   *
   * Returns:
   *   {
   *     "chain": "main" | "testnet" | "signet",
   *     "blocks": 12345,
   *     "headers": 12345,
   *     "best_block_hash": "0x...",
   *     "difficulty": 1234567.89,
   *     "mediantime": 1234567890,
   *     "verification_progress": 0.9999
   *   }
   */
  static std::optional<json> getblockchaininfo(const json& params);

  /**
   * @brief getpeerinfo - Get peer information
   *
   * Params: (none)
   *
   * Returns:
   *   [
   *     {
   *       "id": 1,
   *       "addr": "192.168.1.1:18333",
   *       "addrlocal": "192.168.1.2:54321",
   *       "services": 1,
   *       "lastsend": 1234567890,
   *       "lastrecv": 1234567890,
   *       "bytessent": 1000000,
   *       "bytesrecv": 2000000,
   *       "conntime": 1234567800,
   *       "timeoffset": 5,
   *       "pingtime": 0.5,
   *       "version": 70015,
   *       "user_agent": "/QuantumVault:0.1.0/",
   *       "inbound": false
   *     }
   *   ]
   */
  static std::optional<json> getpeerinfo(const json& params);

  // ============ Mining Methods ============

  /**
   * @brief getmininginfo - Get mining statistics
   *
   * Params: (none)
   *
   * Returns:
   *   {
   *     "blocks": 12345,
   *     "difficulty": 1234567.89,
   *     "network_hash_rate": 123456789,
   *     "pool_valid_rounds": 1000,
   *     "pool_stale_rounds": 10,
   *     "is_generating": false
   *   }
   */
  static std::optional<json> getmininginfo(const json& params);

  /**
   * @brief submitblock - Submit a mined block
   *
   * Params:
   *   - block_hex: string (hex-encoded block)
   *
   * Returns:
   *   {
   *     "status": "accepted" | "rejected",
   *     "reason": "error message if rejected"
   *   }
   */
  static std::optional<json> submitblock(const json& params);

  /**
   * @brief getblocktemplate - Get block template for mining
   *
   * Params: (none)
   *
   * Returns:
   *   {
   *     "version": 1,
   *     "previous_block_hash": "0x...",
   *     "merkle_root": "0x...",
   *     "timestamp": 1234567890,
   *     "difficulty_target": 0x0000ffff,
   *     "nonce_start": 0,
   *     "transactions": [
   *       {
   *         "txid": "0x...",
   *         "version": 1,
   *         "inputs": [...],
   *         "outputs": [...]
   *       }
   *     ]
   *   }
   */
  static std::optional<json> getblocktemplate(const json& params);

  // ============ Utility Methods ============

  /**
   * @brief ping - Ping the server
   *
   * Params: (none)
   *
   * Returns: "pong"
   */
  static std::optional<json> ping(const json& params);

  /**
   * @brief echo - Echo back the parameters
   *
   * Params: (any)
   *
   * Returns: The same parameters
   */
  static std::optional<json> echo(const json& params);

  /**
   * @brief getinfo - Get general server info
   *
   * Params: (none)
   *
   * Returns:
   *   {
   *     "version": "0.1.0",
   *     "protocol_version": 70015,
   *     "wallet_version": 70015,
   *     "balance": 0,
   *     "blocks": 12345,
   *     "timeoffset": 5,
   *     "connections": 8,
   *     "proxy": "",
   *     "difficulty": 1234567.89,
   *     "testnet": false
   *   }
   */
  static std::optional<json> getinfo(const json& params);

  // ============ Admin Methods ============

  /**
   * @brief help - List all available methods
   *
   * Params:
   *   - method: string (optional, filter to specific method)
   *
   * Returns: String or array of method descriptions
   */
  static std::optional<json> help(const json& params);

  /**
   * @brief stop - Shut down the node
   *
   * Params: (none)
   *
   * Returns: "shutting down"
   */
  static std::optional<json> stop(const json& params);
};

}  // namespace qv::rpc
