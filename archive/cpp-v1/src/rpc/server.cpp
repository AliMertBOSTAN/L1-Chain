#include "qv/rpc/server.hpp"

namespace qv::rpc {

// ============ RpcRequest Implementation ============

json RpcRequest::to_json() const {
  json j;
  j["jsonrpc"] = jsonrpc;
  j["method"] = method;
  j["params"] = params;
  j["id"] = id;
  return j;
}

std::optional<RpcRequest> RpcRequest::from_json(const json& data) {
  // TODO: Implement JSON deserialization with validation
  try {
    RpcRequest req;
    req.jsonrpc = data.value("jsonrpc", "2.0");
    req.method = data.value("method", "");
    req.params = data.value("params", json::object());
    req.id = data.value("id", json());

    if (req.method.empty()) {
      return std::nullopt;
    }

    return req;
  } catch (...) {
    return std::nullopt;
  }
}

// ============ RpcResponse Implementation ============

json RpcResponse::Error::to_json() const {
  json j;
  j["code"] = code;
  j["message"] = message;
  if (!data.is_null()) {
    j["data"] = data;
  }
  return j;
}

json RpcResponse::to_json() const {
  json j;
  j["jsonrpc"] = jsonrpc;
  j["id"] = id;

  if (is_error()) {
    j["error"] = error.to_json();
  } else {
    j["result"] = result;
  }

  return j;
}

RpcResponse RpcResponse::error(int code, const std::string& message,
                                const json& id) {
  RpcResponse resp;
  resp.error.code = code;
  resp.error.message = message;
  resp.id = id;
  return resp;
}

RpcResponse RpcResponse::success(const json& result, const json& id) {
  RpcResponse resp;
  resp.result = result;
  resp.error.code = 0;
  resp.id = id;
  return resp;
}

// ============ RpcServer Implementation ============

RpcServer::RpcServer(const ServerConfig& config) : config_(config) {}

RpcServer::~RpcServer() {
  if (running_) {
    stop();
  }
}

std::optional<std::string> RpcServer::start() {
  // TODO: Implement server startup
  // 1. Create TCP socket
  // 2. Bind to config.bind_address:config.port
  // 3. Start listening
  // 4. Spawn thread pool for accepting connections

  running_ = true;
  return std::nullopt;  // Success
}

void RpcServer::stop() {
  // TODO: Implement server shutdown
  // 1. Stop accepting new connections
  // 2. Wait for in-flight requests to complete
  // 3. Close sockets

  running_ = false;
}

bool RpcServer::is_running() const {
  return running_;
}

void RpcServer::register_method(const std::string& method_name,
                                 RpcMethodHandler handler) {
  // TODO: Register method handler
  methods_[method_name] = handler;
}

void RpcServer::unregister_method(const std::string& method_name) {
  // TODO: Unregister method handler
  methods_.erase(method_name);
}

std::size_t RpcServer::method_count() const {
  return methods_.size();
}

std::string RpcServer::handle_request(const std::string& request_json) {
  // TODO: Implement request handling
  try {
    auto request_data = json::parse(request_json);

    // Handle batch requests
    if (request_data.is_array()) {
      auto responses = process_batch(request_data);
      return responses.dump();
    }

    // Handle single request
    auto opt_request = RpcRequest::from_json(request_data);
    if (!opt_request) {
      auto err = RpcResponse::error(-32700, "Parse error");
      return err.to_json().dump();
    }

    auto response = process_request(*opt_request);
    return response.to_json().dump();

  } catch (const std::exception& e) {
    auto err = RpcResponse::error(-32700, std::string("Parse error: ") + e.what());
    return err.to_json().dump();
  }
}

RpcServer::Stats RpcServer::get_stats() const {
  return stats_;
}

RpcResponse RpcServer::process_request(const RpcRequest& request) {
  // TODO: Implement request processing
  // 1. Check authentication if enabled
  // 2. Validate method exists
  // 3. Call handler
  // 4. Return result or error

  stats_.total_requests++;

  auto it = methods_.find(request.method);
  if (it == methods_.end()) {
    stats_.failed_requests++;
    return RpcResponse::error(-32601, "Method not found", request.id);
  }

  try {
    auto result = it->second(request.params);
    if (!result) {
      stats_.failed_requests++;
      return RpcResponse::error(-32603, "Internal error", request.id);
    }

    stats_.successful_requests++;
    return RpcResponse::success(*result, request.id);

  } catch (const std::exception& e) {
    stats_.failed_requests++;
    return RpcResponse::error(-32603,
                             std::string("Internal error: ") + e.what(),
                             request.id);
  }
}

json RpcServer::process_batch(const json& batch) {
  // TODO: Implement batch request processing
  // Process each request in batch and return array of responses
  json responses = json::array();

  for (const auto& req_json : batch) {
    auto opt_request = RpcRequest::from_json(req_json);
    if (!opt_request) {
      responses.push_back(
          RpcResponse::error(-32700, "Parse error").to_json());
      continue;
    }

    auto response = process_request(*opt_request);
    responses.push_back(response.to_json());
  }

  return responses;
}

bool RpcServer::check_auth(const std::string& auth_header) const {
  // TODO: Implement authentication checking
  // Support HTTP Basic Auth or token-based auth
  if (!config_.enable_auth) {
    return true;
  }

  // Placeholder: accept any non-empty auth header
  return !auth_header.empty();
}

}  // namespace qv::rpc
