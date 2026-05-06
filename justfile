# QuantumVault task runner.
# Usage: `just <recipe>` inside `nix develop`.

# List available recipes.
default:
    @just --list

# -- build / test --------------------------------------------------------------

# Build the entire workspace in debug mode.
build:
    cargo build --workspace --all-features

# Build release binaries.
release:
    cargo build --workspace --release --all-features

# Run all unit + integration tests.
test:
    cargo nextest run --workspace --all-features

# Run tests for a single crate.
test-one CRATE:
    cargo nextest run -p {{CRATE}} --all-features

# Run doc tests (nextest does not, by design).
doctest:
    cargo test --doc --workspace

# Run benchmarks.
bench:
    cargo bench --workspace

# -- quality gates -------------------------------------------------------------

# Check formatting without modifying files.
fmt-check:
    cargo fmt --all -- --check

# Auto-format the workspace.
fmt:
    cargo fmt --all

# Run clippy with warnings as errors.
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Security audit of dependencies.
audit:
    cargo audit

# License / source policy check.
deny:
    cargo deny check

# Look for unused workspace dependencies.
machete:
    cargo machete

# Generate API docs for the workspace.
doc:
    cargo doc --workspace --no-deps --all-features

# -- coverage ------------------------------------------------------------------

# Produce an LCOV coverage report (CI-friendly).
cov:
    cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info

# Open an HTML coverage report.
cov-html:
    cargo llvm-cov --workspace --all-features --html
    @echo "Report: target/llvm-cov/html/index.html"

# -- ci ------------------------------------------------------------------------

# Run every gate the CI pipeline runs. No arguments, no surprises.
ci: fmt-check clippy test audit deny
    @echo "✓ CI gate passed."

# -- housekeeping --------------------------------------------------------------

# Clean build artifacts.
clean:
    cargo clean

# Update all dependencies to latest allowed by semver.
update:
    cargo update

# Show outdated dependencies.
outdated:
    cargo outdated --workspace

# -- profiling -----------------------------------------------------------------

# Run performance profiler (see scripts/profile.sh --help).
profile MODE="--help":
    bash scripts/profile.sh {{MODE}}

# Profile criterion benchmarks with flamegraph.
flamegraph-bench:
    bash scripts/profile.sh --bench

# Profile block validation performance.
flamegraph-block:
    bash scripts/profile.sh --block

# Profile full node startup.
flamegraph-node:
    bash scripts/profile.sh --node

# -- documentation -------------------------------------------------------------

# Build the mdBook documentation site.
docs:
    mdbook build
    @echo "Book: book/build/index.html"

# Serve the mdBook site locally with live reload.
docs-serve:
    mdbook serve --open
