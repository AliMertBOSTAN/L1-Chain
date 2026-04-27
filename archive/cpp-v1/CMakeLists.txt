cmake_minimum_required(VERSION 3.20)
project(QuantumVault VERSION 0.1.0 LANGUAGES CXX)

# Set C++23 standard (required for std::expected)
set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Set default build type
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Debug)
endif()

# Include CMake modules for sanitizers, coverage, and static analysis
list(APPEND CMAKE_MODULE_PATH ${CMAKE_CURRENT_SOURCE_DIR}/cmake)
include(sanitizers)
include(coverage)
include(static_analysis)

# Output directories
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)

# Compiler flags
if(MSVC)
  add_compile_options(/W4 /WX)
else()
  add_compile_options(-Wall -Wextra -Wpedantic -Werror)
  if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    add_compile_options(-g -O0)
  else()
    add_compile_options(-O3 -march=native)
  endif()
endif()

# Find required packages
find_package(PkgConfig REQUIRED)
find_package(Threads REQUIRED)

# Protobuf
find_package(Protobuf REQUIRED)

# spdlog
find_package(spdlog REQUIRED)

# gtest
find_package(GTest REQUIRED)

# Google Benchmark
find_package(benchmark REQUIRED)

# liboqs (Post-Quantum Cryptography)
pkg_check_modules(LIBOQS REQUIRED IMPORTED_TARGET liboqs)

# leveldb or rocksdb
pkg_check_modules(LEVELDB IMPORTED_TARGET leveldb)
pkg_check_modules(ROCKSDB IMPORTED_TARGET rocksdb)

if(NOT LEVELDB_FOUND AND NOT ROCKSDB_FOUND)
  message(FATAL_ERROR "Either leveldb or rocksdb must be installed")
endif()

# OpenSSL (for libp2p or custom networking)
find_package(OpenSSL REQUIRED)

# Boost (optional, for utilities)
find_package(Boost QUIET)

# Set include directories
include_directories(${CMAKE_CURRENT_SOURCE_DIR}/src)

# Enable testing
enable_testing()

# Core UTXO library
add_library(qv_core STATIC
    src/core/transaction.cpp
    src/core/block.cpp
    src/core/utxo.cpp
)

target_include_directories(qv_core PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_compile_features(qv_core PUBLIC cxx_std_20)

# Testing Configuration
set(ENABLE_TESTING ON CACHE BOOL "Enable testing")
set(ENABLE_COVERAGE OFF CACHE BOOL "Enable code coverage reporting")

# Code Coverage (optional)
if(ENABLE_COVERAGE AND CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
  message(STATUS "Code coverage reporting ENABLED")
  add_compile_options(--coverage -O0)
  add_link_options(--coverage)

  # Create coverage target
  find_program(LCOV lcov)
  if(LCOV)
    add_custom_target(test_coverage
      COMMAND ${CMAKE_CTEST_COMMAND}
      COMMAND ${LCOV} --directory ${CMAKE_BINARY_DIR} --capture --output-file coverage.info
      COMMAND ${LCOV} --remove coverage.info '/usr/*' '*/tests/*' --output-file coverage_filtered.info
      COMMAND genhtml coverage_filtered.info --output-directory ${CMAKE_BINARY_DIR}/coverage
      WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
      COMMENT "Generating code coverage report..."
    )
  endif()
endif()

# Sanitizers (optional, for Debug builds)
option(ENABLE_SANITIZERS "Enable address and undefined behavior sanitizers" OFF)
if(ENABLE_SANITIZERS AND CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
  message(STATUS "Sanitizers ENABLED")
  add_compile_options(-fsanitize=address -fsanitize=undefined -fno-omit-frame-pointer)
  add_link_options(-fsanitize=address -fsanitize=undefined)
endif()

# Add subdirectories
add_subdirectory(src)
add_subdirectory(tests)
add_subdirectory(tools)

# Print configuration summary
message(STATUS "")
message(STATUS "QuantumVault Configuration Summary")
message(STATUS "==================================")
message(STATUS "CMAKE_BUILD_TYPE: ${CMAKE_BUILD_TYPE}")
message(STATUS "CMAKE_CXX_COMPILER: ${CMAKE_CXX_COMPILER}")
message(STATUS "CMAKE_CXX_FLAGS: ${CMAKE_CXX_FLAGS}")
message(STATUS "CMAKE_CXX_STANDARD: ${CMAKE_CXX_STANDARD}")
message(STATUS "")
message(STATUS "Dependencies:")
message(STATUS "  Protobuf version: ${Protobuf_VERSION}")
message(STATUS "  spdlog: ${spdlog_FOUND}")
message(STATUS "  GTest: ${GTest_FOUND}")
message(STATUS "  Google Benchmark: ${benchmark_FOUND}")
message(STATUS "  liboqs: ${LIBOQS_FOUND}")
if(LEVELDB_FOUND)
  message(STATUS "  leveldb: TRUE")
else()
  message(STATUS "  rocksdb: TRUE")
endif()
message(STATUS "  OpenSSL version: ${OPENSSL_VERSION}")
message(STATUS "")
message(STATUS "Testing & Quality:")
message(STATUS "  Testing: ENABLED")
message(STATUS "  Code Coverage: ${ENABLE_COVERAGE}")
message(STATUS "  Sanitizers: ${ENABLE_SANITIZERS}")
message(STATUS "")
message(STATUS "Build Targets:")
message(STATUS "  ninja -C build                    # Build all")
message(STATUS "  ctest --test-dir build            # Run tests")
message(STATUS "  ctest --test-dir build -R crypto  # Run crypto tests")
message(STATUS "  ninja -C build test_coverage      # Generate coverage (if enabled)")
message(STATUS "")
message(STATUS "Configure with coverage:")
message(STATUS "  cmake --preset dev -DENABLE_COVERAGE=ON")
message(STATUS "")
