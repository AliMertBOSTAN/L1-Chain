#include <iostream>
#include <string>
#include <memory>
#include <csignal>
#include <atomic>

namespace qv {

// Forward declarations for component initialization
class Storage;
class UTXOSet;
class ChainState;
class Network;
class Consensus;
class RPCServer;

// Global flag for graceful shutdown
std::atomic<bool> shutdown_requested{false};

void signal_handler(int signal) {
    if (signal == SIGINT || signal == SIGTERM) {
        std::cout << "\nShutdown signal received, stopping node gracefully..." << std::endl;
        shutdown_requested.store(true);
    }
}

struct NodeConfig {
    std::string config_file;
    std::string data_dir;
    uint16_t rpc_port;
    uint16_t p2p_port;
};

class QuantumVaultNode {
public:
    explicit QuantumVaultNode(const NodeConfig& config)
        : config_(config),
          storage_(nullptr),
          utxo_set_(nullptr),
          chain_state_(nullptr),
          network_(nullptr),
          consensus_(nullptr),
          rpc_server_(nullptr) {}

    bool initialize() {
        std::cout << "[Node] Initializing QuantumVault node..." << std::endl;

        // Initialize storage subsystem
        std::cout << "[Node] Initializing storage from: " << config_.data_dir << std::endl;
        // TODO: storage_ = std::make_unique<Storage>(config_.data_dir);

        // Initialize UTXO set
        std::cout << "[Node] Initializing UTXO set..." << std::endl;
        // TODO: utxo_set_ = std::make_unique<UTXOSet>(storage_.get());

        // Initialize chain state
        std::cout << "[Node] Initializing chain state..." << std::endl;
        // TODO: chain_state_ = std::make_unique<ChainState>(storage_.get());

        // Initialize network subsystem
        std::cout << "[Node] Initializing network (P2P port: " << config_.p2p_port << ")..." << std::endl;
        // TODO: network_ = std::make_unique<Network>(config_.p2p_port);

        // Initialize consensus engine
        std::cout << "[Node] Initializing consensus engine..." << std::endl;
        // TODO: consensus_ = std::make_unique<Consensus>(chain_state_.get(), utxo_set_.get());

        // Initialize RPC server
        std::cout << "[Node] Initializing RPC server (port: " << config_.rpc_port << ")..." << std::endl;
        // TODO: rpc_server_ = std::make_unique<RPCServer>(config_.rpc_port, consensus_.get(), chain_state_.get());

        std::cout << "[Node] Initialization complete." << std::endl;
        return true;
    }

    void run() {
        std::cout << "[Node] Starting event loop..." << std::endl;

        // Install signal handlers for graceful shutdown
        std::signal(SIGINT, signal_handler);
        std::signal(SIGTERM, signal_handler);

        // Main event loop
        while (!shutdown_requested.load()) {
            // TODO: Process network messages
            // TODO: Validate incoming transactions
            // TODO: Build new blocks
            // TODO: Update consensus state
            // TODO: Handle RPC requests

            // Placeholder sleep to prevent busy-wait
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
        }

        std::cout << "[Node] Event loop stopped." << std::endl;
    }

    void shutdown() {
        std::cout << "[Node] Shutting down components..." << std::endl;

        // TODO: Stop RPC server
        // TODO: Stop network
        // TODO: Flush storage
        // TODO: Cleanup consensus state

        std::cout << "[Node] Shutdown complete." << std::endl;
    }

private:
    NodeConfig config_;
    std::unique_ptr<Storage> storage_;
    std::unique_ptr<UTXOSet> utxo_set_;
    std::unique_ptr<ChainState> chain_state_;
    std::unique_ptr<Network> network_;
    std::unique_ptr<Consensus> consensus_;
    std::unique_ptr<RPCServer> rpc_server_;
};

NodeConfig parse_arguments(int argc, char* argv[]) {
    NodeConfig config;
    config.config_file = "";
    config.data_dir = "./quantum_vault_data";
    config.rpc_port = 8332;
    config.p2p_port = 8333;

    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];

        if (arg == "--config" && i + 1 < argc) {
            config.config_file = argv[++i];
        } else if (arg == "--datadir" && i + 1 < argc) {
            config.data_dir = argv[++i];
        } else if (arg == "--rpcport" && i + 1 < argc) {
            config.rpc_port = static_cast<uint16_t>(std::stoi(argv[++i]));
        } else if (arg == "--p2pport" && i + 1 < argc) {
            config.p2p_port = static_cast<uint16_t>(std::stoi(argv[++i]));
        } else if (arg == "--help") {
            std::cout << "QuantumVault Node Usage:" << std::endl;
            std::cout << "  --config <path>    Path to configuration file" << std::endl;
            std::cout << "  --datadir <path>   Data directory (default: ./quantum_vault_data)" << std::endl;
            std::cout << "  --rpcport <port>   RPC server port (default: 8332)" << std::endl;
            std::cout << "  --p2pport <port>   P2P network port (default: 8333)" << std::endl;
            std::cout << "  --help              Show this message" << std::endl;
            std::exit(0);
        }
    }

    return config;
}

} // namespace qv

int main(int argc, char* argv[]) {
    std::cout << "QuantumVault Full Node v1.0" << std::endl;
    std::cout << "=============================" << std::endl;

    qv::NodeConfig config = qv::parse_arguments(argc, argv);

    std::cout << "Configuration:" << std::endl;
    std::cout << "  Config file: " << (config.config_file.empty() ? "(none)" : config.config_file) << std::endl;
    std::cout << "  Data dir: " << config.data_dir << std::endl;
    std::cout << "  RPC port: " << config.rpc_port << std::endl;
    std::cout << "  P2P port: " << config.p2p_port << std::endl;

    qv::QuantumVaultNode node(config);

    if (!node.initialize()) {
        std::cerr << "Failed to initialize node" << std::endl;
        return 1;
    }

    node.run();
    node.shutdown();

    return 0;
}
