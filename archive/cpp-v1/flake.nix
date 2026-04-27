{
  description = "QuantumVault - L1 Blockchain with Post-Quantum Cryptography";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Build tools
            cmake
            ninja
            pkg-config

            # Compilers
            gcc
            clang
            llvm

            # C++ Standard Library
            libcxx
            stdenv.cc.cc.lib

            # Core dependencies
            protobuf
            spdlog
            gtest
            gbenchmark

            # Database
            leveldb
            rocksdb

            # Post-Quantum Cryptography
            liboqs

            # Networking (if custom implementation, these are standard deps)
            openssl
            boost

            # Development tools
            gdb
            valgrind
            ccache
            cppcheck
            clang-tools

            # Version control
            git
          ];

          shellHook = ''
            export CMAKE_BUILD_TYPE=Debug
            export CXXFLAGS="-std=c++20 -fPIC"
            echo "QuantumVault Development Environment"
            echo "CMAKE_BUILD_TYPE: $CMAKE_BUILD_TYPE"
            echo "Run 'cmake --preset dev' to configure the build"
          '';
        };

        packages.default = pkgs.stdenv.mkDerivation {
          name = "quantumvault";
          src = ./.;

          buildInputs = with pkgs; [
            cmake
            ninja
            pkg-config
            gcc
            protobuf
            spdlog
            gtest
            gbenchmark
            leveldb
            rocksdb
            liboqs
            openssl
            boost
          ];

          buildPhase = ''
            mkdir -p build
            cd build
            cmake -GNinja -DCMAKE_BUILD_TYPE=Release ..
            ninja
          '';

          installPhase = ''
            mkdir -p $out/bin $out/lib $out/include
            ninja install DESTDIR=$out
          '';
        };
      }
    );
}
