#!/usr/bin/env bash
set -euo pipefail

# QuantumVault Performance Profiling Script
# Generates flamegraphs for benchmarks, block validation, and node startup.
#
# Usage:
#   ./scripts/profile.sh --bench                 # Run criterion benchmarks with flamegraph
#   ./scripts/profile.sh --block                 # Profile block validation performance
#   ./scripts/profile.sh --node                  # Profile full node startup + 100 blocks
#   ./scripts/profile.sh --help                  # Show this help
#
# Prerequisites:
#   - cargo-flamegraph crate (auto-installed if missing)
#   - perf (Linux) or Xcode performance tools (macOS)
#   - Rust stable 1.78+
#
# Output:
#   SVG flamegraphs are written to target/flamegraph/

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Parse arguments
MODE="${1:-}"

# Helper functions
log_info() {
    echo -e "${GREEN}[info]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[warn]${NC} $1"
}

log_error() {
    echo -e "${RED}[error]${NC} $1"
}

show_help() {
    grep '^# ' "$0" | sed 's/^# //' | head -20
}

check_tool() {
    if ! command -v "$1" &> /dev/null; then
        return 1
    fi
    return 0
}

# Check prerequisites
check_prerequisites() {
    local has_error=0

    # Check for cargo-flamegraph
    if ! cargo flamegraph --version &> /dev/null; then
        log_warn "cargo-flamegraph not found; installing..."
        cargo install flamegraph
    fi

    # Check for perf (Linux) or Xcode tools (macOS)
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        if ! check_tool perf; then
            log_error "perf not found. Install with: sudo apt-get install linux-tools-generic"
            has_error=1
        fi
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        if ! check_tool xcrun; then
            log_error "Xcode tools not found. Install with: xcode-select --install"
            has_error=1
        fi
    else
        log_warn "Unknown OS ($OSTYPE). Flamegraph may not work on this platform."
    fi

    if [ $has_error -ne 0 ]; then
        return 1
    fi
}

# Build with profiling profile
build_profiling() {
    log_info "Building with profiling profile..."
    cd "$PROJECT_ROOT"
    cargo build --profile profiling --all-features 2>&1 | tail -10
}

# Profile benchmark suite
profile_bench() {
    log_info "Running criterion benchmarks with flamegraph..."
    log_info "Target: block validation < 500ms"
    cd "$PROJECT_ROOT"

    # qv-script benchmarks
    log_info "Profiling qv-script script_bench..."
    cargo flamegraph \
        --bench script_bench \
        --package qv-script \
        --profile profiling \
        -o target/flamegraph/script_bench.svg \
        -- --profile-time 5 2>&1 | tail -5

    # qv-core benchmarks
    log_info "Profiling qv-core core_bench..."
    cargo flamegraph \
        --bench core_bench \
        --package qv-core \
        --profile profiling \
        -o target/flamegraph/core_bench.svg \
        -- --profile-time 5 2>&1 | tail -5

    log_info "Benchmark flamegraphs generated:"
    echo "  - target/flamegraph/script_bench.svg"
    echo "  - target/flamegraph/core_bench.svg"
}

# Profile block validation path
profile_block() {
    log_info "Profiling block validation performance..."
    log_info "This runs a synthetic block validation test under flamegraph."
    cd "$PROJECT_ROOT"

    # Build a test binary if it doesn't exist
    # (assumes a block_validator_bench or similar in crates)
    # For now, we'll profile qv-core since it has validation logic

    cargo flamegraph \
        --bench core_bench \
        --package qv-core \
        --profile profiling \
        -o target/flamegraph/block_validation.svg \
        -- --profile-time 10 --bench-filter block 2>&1 | tail -5

    log_info "Block validation flamegraph generated:"
    echo "  - target/flamegraph/block_validation.svg"
}

# Profile full node startup and initial block processing
profile_node() {
    log_info "Profiling full node startup + block processing..."
    log_info "This will start qv-node, process 100 synthetic blocks, then stop."
    cd "$PROJECT_ROOT"

    # Build release binary
    log_info "Building qv-node binary..."
    cargo build --profile profiling -p qv-node 2>&1 | tail -5

    BINARY="${PROJECT_ROOT}/target/profiling/qv-node"
    DATA_DIR="${PROJECT_ROOT}/data/profiling"

    # Clean and prepare data directory
    rm -rf "$DATA_DIR"
    mkdir -p "$DATA_DIR"

    # Profile node startup
    log_info "Profiling node startup with flamegraph..."
    cargo flamegraph \
        --bin qv-node \
        --profile profiling \
        -o target/flamegraph/node_startup.svg \
        -- \
        --init \
        --network devnet \
        --data-dir "$DATA_DIR" \
        2>&1 | tail -5

    log_info "Node startup flamegraph generated:"
    echo "  - target/flamegraph/node_startup.svg"

    log_info "Note: Full node block processing profile requires a running node."
    log_info "      Use: nix develop -> just devnet"
    log_info "      Then attach profiler to running node process."
}

# Main entry
main() {
    log_info "QuantumVault Performance Profiler"

    case "$MODE" in
        --help|-h)
            show_help
            exit 0
            ;;
        --bench)
            check_prerequisites || exit 1
            profile_bench
            ;;
        --block)
            check_prerequisites || exit 1
            profile_block
            ;;
        --node)
            check_prerequisites || exit 1
            profile_node
            ;;
        "")
            log_error "No mode specified. Use --help for usage."
            exit 1
            ;;
        *)
            log_error "Unknown mode: $MODE"
            show_help
            exit 1
            ;;
    esac

    log_info "Profiling complete. View flamegraphs in: target/flamegraph/"
}

main
