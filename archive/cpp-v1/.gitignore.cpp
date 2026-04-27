# CMake build directories
build/
build-*/
cmake-build-*/
CMakeFiles/
CMakeCache.txt
CMakeOutput.log
cmake_install.cmake
Makefile

# Ninja build files
.ninja_deps
.ninja_log
compile_commands.json

# IDE and Editor
.vscode/
.idea/
*.swp
*.swo
*~
*.sublime-*
.DS_Store
.clang-format.swp

# Object files and archives
*.o
*.obj
*.a
*.lib
*.so
*.so.*
*.dylib
*.exe
*.dll

# Compiled output
*.out
*.elf

# Dependency directories
.deps/
vendor/

# Test and coverage
*.gcda
*.gcno
coverage/
lcov/
*.profdata

# Generated files
*.pb.h
*.pb.cc
*.pb.py
*.pb2.py
protobuf_outputs/

# Package managers
conan/
vcpkg_installed/

# Temporary files
*.tmp
*.temp
*.log
*.lfb

# OS-specific
Thumbs.db
.DS_Store
.AppleDouble
.LSOverride

# Python
__pycache__/
*.py[cod]
*$py.class
*.so
.Python
env/
venv/
.env

# Node (if used for build tools)
node_modules/
npm-debug.log
yarn-error.log

# Doxygen
docs/html/
docs/latex/
docs/rtf/

# Code coverage
coverage/
*.coverage
.coverage

# Clang tools
.clang-tidy

# Debugging
.gdb_history
*.debug
vgcore.*

# Blockchain-specific
chaindata/
db/
mempool/
*.chain
*.dat
keystore/
private_keys/
wallets/
node_data/
.node_data/

# Test artifacts
test_results/
gtest_results/

# Benchmarks
benchmark_results/
*.benchstat

# Lock files (optional - can commit or exclude based on preference)
# Uncomment to ignore lock files
# *.lock

# Build artifacts directory structure
bin/
lib/
include/

# Install directory
install_manifest.txt

# Backup files
*.bak
*.backup

# Misc
.cache/
.ccache/
ccache/
