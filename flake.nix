{
  description = "QuantumVault L1 — quantum-resistant UTXO blockchain (Rust)";

  inputs = {
    nixpkgs.url       = "github:NixOS/nixpkgs/nixos-24.05";
    flake-utils.url   = "github:numtide/flake-utils";
    # Pinned Rust toolchain driven by rust-toolchain.toml
    rust-overlay = {
      url              = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Use the toolchain pinned in rust-toolchain.toml
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile
          ./rust-toolchain.toml;

        # Common native deps every crate may want to link
        nativeBuildInputs = with pkgs; [
          pkg-config
          clang
          cmake
          perl
        ];

        buildInputs = with pkgs; [
          openssl
          liboqs          # PQC primitives (Dilithium, Kyber backing lib)
          rocksdb
          zlib
          bzip2
          lz4
          snappy
          zstd
          protobuf        # for any proto-codec cases
        ];

        devTools = with pkgs; [
          rustToolchain
          rust-analyzer

          # Rust ecosystem tools
          cargo-audit
          cargo-deny
          cargo-outdated
          cargo-edit
          cargo-watch
          cargo-nextest
          cargo-llvm-cov
          cargo-flamegraph
          cargo-fuzz
          cargo-expand
          cargo-bloat
          cargo-machete

          # Task runner
          just

          # Docs
          mdbook
          graphviz

          # Debug / perf
          gdb
          valgrind

          # Git hooks helper
          pre-commit
        ];
      in {
        devShells.default = pkgs.mkShell {
          inherit nativeBuildInputs buildInputs;

          packages = devTools;

          # Environment
          RUST_BACKTRACE = "1";
          RUST_LOG       = "info,qv=debug";

          # Point openssl-sys / pqcrypto to the system libraries
          OPENSSL_DIR     = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
          OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";

          # liboqs path for pqcrypto to use an external shared library if desired
          LIBOQS_DIR     = "${pkgs.liboqs}";

          # RocksDB prebuilt — skip rocksdb-sys recompilation
          ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib";
          ROCKSDB_INCLUDE_DIR = "${pkgs.rocksdb}/include";

          shellHook = ''
            echo "────────────────────────────────────────────────────────────"
            echo " QuantumVault devshell"
            echo "   rustc: $(rustc --version)"
            echo "   cargo: $(cargo --version)"
            echo "   liboqs: ${pkgs.liboqs.version}"
            echo "   openssl: ${pkgs.openssl.version}"
            echo "   rocksdb: ${pkgs.rocksdb.version}"
            echo ""
            echo " Handy commands:"
            echo "   just build   # cargo build --workspace"
            echo "   just test    # cargo nextest run"
            echo "   just ci      # fmt + clippy + test + audit"
            echo "────────────────────────────────────────────────────────────"
          '';
        };

        # Optional: expose a workspace build check as a flake-output package
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname   = "quantumvault";
          version = "0.1.0";
          src     = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          inherit nativeBuildInputs buildInputs;
          doCheck = false;  # Unit tests run through CI, not nix build
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
