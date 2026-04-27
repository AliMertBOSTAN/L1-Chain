#pragma once

#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <functional>
#include <nlohmann/json.hpp>

namespace qv::rpc {

using json = nlohmann::json;

/**
 * @brief RPC Server configuration
 */
struct ServerConfig {
  /**
   * @brief Server port
   */
  std::uint16_t port = 8332;

  /**
   * @brief Bind address (e.g., "127.0.0.1", "0.0.0.0")
   */
  std::string bind_address = "127.0.0.1";

  /**
   * @brief Maximum concurrent connections
   */
  std::uint16_t max_connections = 256;

  /**
   * @brief Request timeout in seconds
   */
  std::uint32_t request_timeout = 30;

  /**
   * @brief Enable authentication (basic auth or token)
   */
  bool enable_auth = false;

  /**
   * @brief Authentication token
   */
  std::string auth_token;
};

/**
 * @brief RPC Request representation
 */
struct RpcRequest {
  /**
   * @brief JSON-RPC version (should be "2.0")
   */
  std::string jsonrpc = "2.0";

  /**
   * @brief Method name
   */
  std::string method;

  /**
   * @brief Method parameters
   */
  json params;

  /**
   * @brief Request ID (for matching responses)
   */
  json id;

  /**
   * @brief Serialize to JSON
   */
  json to_json() const;

  /**
   * @brief Deserialize from JSON
   */
  static std::optional<RpcRequest> from_json(const json& data);
};

/**
 * @brief RPC Response representation
 */
struct RpcResponse {
  /**
   * @brief JSON-RPC version
   */
  std::string jsonrpc = "2.0";

  /**
   * @brief Request ID (to match with request)
   */
  json id;

  /**
   * @brief Result (if successful)
   */
  json result;

  /**
   * @brief Error (if failed)
   */
  struct Error {
    int code = 0;
    std::string message;
    json data;

    json to_json() const;
  } error;

  /**
   * @brief Is this an error response?
   */
  bool is_error() const { return error.code != 0; }

  /**
   * @brief Serialize to JSON
   */
  json to_json() const;

  /**
   * @brief Create error response
   */
  static RpcResponse error(int code, const std::string& message,
                           const json& id = json());

  /**
   * @brief Create success response
   */
  static RpcResponse success(const json& result, const json& id = json());
};

/**
 * @brief RPC Method handler function type
 *
 * Takes (params, returns result or error)
 */
using RpcMethodHandler = std::function<std::optional<json>(const json&)>;

/**
 * @brief JSON-RPC 2.0 Server
 *
 * Implements JSON-RPC 2.0 specification:
 * - Request/response model
 * - Batch requests
 * - Named and positional parameters
 * - Error codes (-32600 to -32099 for protocol errors)
 */
class RpcServer {
 public:
  /**
   * @brief Construct RPC server with configuration
   * @param config Server configuration
   */
  explicit RpcServer(const ServerConfig& config = ServerConfig{});

  ~RpcServer();

  /**
   * @brief Start the server
   *
   * Binds to configured address and port, begins accepting connections.
   *
   * @return Result: success or error message
   */
  std::optional<std::string> start();

  /**
   * @brief Stop the server
   *
   * Gracefully shuts down, waiting for in-flight requests.
   */
  void stop();

  /**
   * @brief Check if server is running
   */
  bool is_running() const;

  /**
   * @brief Register an RPC method
   *
   * @param method_name The method name (e.g., "getblock")
   * @param handler The handler function
   *
   * Example:
   *   server.register_method("getblock", [](const json& params) {
   *     // params[0] is the block hash
   *     return json{{"hash", "...", "height", 12345}};
   *   });
   */
  void register_method(const std::string& method_name,
                       RpcMethodHandler handler);

  /**
   * @brief Unregister an RPC method
   */
  void unregister_method(const std::string& method_name);

  /**
   * @brief Get registered method count
   */
  std::size_t method_count() const;

  /**
   * @brief Handle a raw JSON-RPC request
   *
   * Internal use for processing requests from network.
   *
   * @param request_json The request as JSON string
   * @return Response JSON string
   */
  std::string handle_request(const std::string& request_json);

  /**
   * @brief Get server statistics
   */
  struct Stats {
    std::uint32_t total_requests = 0;
    std::uint32_t successful_requests = 0;
    std::uint32_t failed_requests = 0;
    std::uint32_t current_connections = 0;
  };

  Stats get_stats() const;

 private:
  /**
   * @brief Handle a single RPC request
   */
  RpcResponse process_request(const RpcRequest& request);

  /**
   * @brief Handle batch requests
   */
  json process_batch(const json& batch);

  /**
   * @brief Validate authentication
   */
  bool check_auth(const std::string& auth_header) const;

  /**
   * @brief State
   */
  ServerConfig config_;
  std::map<std::string, RpcMethodHandler> methods_;
  bool running_ = false;
  Stats stats_;

  // TODO: Add actual networking (boost::asio, asio, or similar)
};

}  // namespace qv::rpc
