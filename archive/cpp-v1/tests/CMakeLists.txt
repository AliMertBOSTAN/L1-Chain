# QuantumVault Test Suite Configuration
# Unit tests, integration tests, and benchmarks for the L1 blockchain
# See docs/TESTING_STRATEGY.md for comprehensive testing strategy

include_directories(${CMAKE_CURRENT_SOURCE_DIR}/../include)
include_directories(${CMAKE_CURRENT_SOURCE_DIR}/../src)

# ============================================================================
# Helper Functions for Test and Benchmark Creation
# ============================================================================

# Create a test executable with proper dependencies
function(add_qv_test TEST_NAME SOURCE_FILES DEPENDENCIES)
  add_executable(${TEST_NAME} ${SOURCE_FILES})

  target_link_libraries(${TEST_NAME}
    ${DEPENDENCIES}
    GTest::gtest
    GTest::gtest_main
    Threads::Threads
  )

  target_include_directories(${TEST_NAME} PRIVATE
    ${CMAKE_SOURCE_DIR}/src
    ${CMAKE_CURRENT_SOURCE_DIR}
  )

  # Register with ctest and set timeout
  gtest_discover_tests(${TEST_NAME}
    WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
    PROPERTIES
      TIMEOUT 300
  )

  # Add labels for filtering
  set_tests_properties(${TEST_NAME} PROPERTIES
    LABELS "unit"
  )
endfunction()

# Create a benchmark executable
function(add_qv_benchmark BENCH_NAME SOURCE_FILES DEPENDENCIES)
  add_executable(${BENCH_NAME} ${SOURCE_FILES})

  target_link_libraries(${BENCH_NAME}
    ${DEPENDENCIES}
    benchmark::benchmark
    Threads::Threads
  )

  target_include_directories(${BENCH_NAME} PRIVATE
    ${CMAKE_SOURCE_DIR}/src
    ${CMAKE_CURRENT_SOURCE_DIR}
  )
endfunction()

# Create integration test
function(add_qv_integration_test TEST_NAME SOURCE_FILES DEPENDENCIES)
  add_qv_test(${TEST_NAME} "${SOURCE_FILES}" "${DEPENDENCIES}")
  set_tests_properties(${TEST_NAME} PROPERTIES
    LABELS "integration"
    TIMEOUT 600
  )
endfunction()

# ============================================================================
# UNIT TESTS - Security-critical modules
# ============================================================================

# Crypto Module: PQC signatures (Dilithium) and KEM (Kyber)
# Coverage target: 95%
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/crypto/test_dilithium.cpp OR
   EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/crypto/test_kyber.cpp OR
   EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/crypto)
  file(GLOB CRYPTO_TEST_SOURCES "crypto/test_*.cpp")
  if(CRYPTO_TEST_SOURCES)
    add_qv_test(test_crypto
      ${CRYPTO_TEST_SOURCES}
      "qv_crypto"
    )
  endif()
endif()

# Core Module: UTXO, transactions, blocks
# Coverage target: 90%
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/core)
  file(GLOB CORE_TEST_SOURCES "core/test_*.cpp")
  if(CORE_TEST_SOURCES)
    add_qv_test(test_core
      ${CORE_TEST_SOURCES}
      "qv_core;qv_crypto"
    )
  endif()
endif()

# Consensus Module: PoW and PoS
# Coverage target: 85%
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/consensus)
  file(GLOB CONSENSUS_TEST_SOURCES "consensus/test_*.cpp")
  if(CONSENSUS_TEST_SOURCES)
    add_qv_test(test_consensus
      ${CONSENSUS_TEST_SOURCES}
      "qv_consensus;qv_core;qv_crypto"
    )
  endif()
endif()

# Privacy Module: Stealth addresses
# Coverage target: 85%
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/privacy)
  file(GLOB PRIVACY_TEST_SOURCES "privacy/test_*.cpp")
  if(PRIVACY_TEST_SOURCES)
    add_qv_test(test_privacy
      ${PRIVACY_TEST_SOURCES}
      "qv_privacy;qv_crypto;qv_core"
    )
  endif()
endif()

# VM Module: DSL bytecode interpreter
# Coverage target: 90%
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/vm)
  file(GLOB VM_TEST_SOURCES "vm/test_*.cpp")
  if(VM_TEST_SOURCES)
    add_qv_test(test_vm
      ${VM_TEST_SOURCES}
      "qv_vm;qv_core;qv_crypto"
    )
  endif()
endif()

# Storage Module: Mempool, block store, UTXO store
# Coverage target: 80%
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/storage)
  file(GLOB STORAGE_TEST_SOURCES "storage/test_*.cpp")
  if(STORAGE_TEST_SOURCES)
    add_qv_test(test_storage
      ${STORAGE_TEST_SOURCES}
      "qv_storage;qv_core;qv_crypto"
    )
  endif()
endif()

# Data Availability Module: Erasure coding, proofs
# Coverage target: 85%
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/da)
  file(GLOB DA_TEST_SOURCES "da/test_*.cpp")
  if(DA_TEST_SOURCES)
    add_qv_test(test_da
      ${DA_TEST_SOURCES}
      "qv_da;qv_crypto;qv_core"
    )
  endif()
endif()

# Network Module: P2P, libp2p
# Coverage target: 75%
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/net)
  file(GLOB NET_TEST_SOURCES "net/test_*.cpp")
  if(NET_TEST_SOURCES)
    add_qv_test(test_net
      ${NET_TEST_SOURCES}
      "qv_net;qv_core;qv_crypto"
    )
  endif()
endif()

# RPC Module: JSON-RPC API
# Coverage target: 70%
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/rpc)
  file(GLOB RPC_TEST_SOURCES "rpc/test_*.cpp")
  if(RPC_TEST_SOURCES)
    add_qv_test(test_rpc
      ${RPC_TEST_SOURCES}
      "qv_rpc;qv_core;qv_crypto"
    )
  endif()
endif()

# ============================================================================
# INTEGRATION TESTS - End-to-end workflows
# ============================================================================

if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/integration)
  file(GLOB INTEGRATION_TEST_SOURCES "integration/test_*.cpp")
  if(INTEGRATION_TEST_SOURCES)
    add_qv_integration_test(test_integration
      ${INTEGRATION_TEST_SOURCES}
      "qv_core;qv_crypto;qv_consensus;qv_privacy;qv_vm;qv_storage;qv_da;qv_net"
    )
  endif()
endif()

# ============================================================================
# BENCHMARKS - Performance measurement
# ============================================================================

file(MAKE_DIRECTORY ${CMAKE_BINARY_DIR}/results)

# Crypto benchmarks: Sign/verify throughput, KEM performance, hash rates
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/crypto/bench_dilithium.cpp OR
   EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/crypto/bench_kyber.cpp OR
   EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/crypto/bench_hash.cpp)
  file(GLOB CRYPTO_BENCH_SOURCES "crypto/bench_*.cpp")
  if(CRYPTO_BENCH_SOURCES)
    add_qv_benchmark(bench_crypto ${CRYPTO_BENCH_SOURCES} "qv_crypto")
    add_custom_target(run_bench_crypto
      COMMAND ${CMAKE_BINARY_DIR}/bin/bench_crypto --benchmark_format=json
              > ${CMAKE_BINARY_DIR}/results/bench_crypto.json
      DEPENDS bench_crypto
      COMMENT "Running crypto benchmarks..."
    )
  endif()
endif()

# Core benchmarks: Transaction/block processing
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/core)
  file(GLOB CORE_BENCH_SOURCES "core/bench_*.cpp")
  if(CORE_BENCH_SOURCES)
    add_qv_benchmark(bench_core ${CORE_BENCH_SOURCES} "qv_core;qv_crypto")
    add_custom_target(run_bench_core
      COMMAND ${CMAKE_BINARY_DIR}/bin/bench_core --benchmark_format=json
              > ${CMAKE_BINARY_DIR}/results/bench_core.json
      DEPENDS bench_core
      COMMENT "Running core benchmarks..."
    )
  endif()
endif()

# Consensus benchmarks: PoW/PoS validation
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/consensus)
  file(GLOB CONSENSUS_BENCH_SOURCES "consensus/bench_*.cpp")
  if(CONSENSUS_BENCH_SOURCES)
    add_qv_benchmark(bench_consensus ${CONSENSUS_BENCH_SOURCES} "qv_consensus;qv_core;qv_crypto")
    add_custom_target(run_bench_consensus
      COMMAND ${CMAKE_BINARY_DIR}/bin/bench_consensus --benchmark_format=json
              > ${CMAKE_BINARY_DIR}/results/bench_consensus.json
      DEPENDS bench_consensus
      COMMENT "Running consensus benchmarks..."
    )
  endif()
endif()

# Storage benchmarks: Mempool, UTXO operations
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/storage)
  file(GLOB STORAGE_BENCH_SOURCES "storage/bench_*.cpp")
  if(STORAGE_BENCH_SOURCES)
    add_qv_benchmark(bench_storage ${STORAGE_BENCH_SOURCES} "qv_storage;qv_core")
    add_custom_target(run_bench_storage
      COMMAND ${CMAKE_BINARY_DIR}/bin/bench_storage --benchmark_format=json
              > ${CMAKE_BINARY_DIR}/results/bench_storage.json
      DEPENDS bench_storage
      COMMENT "Running storage benchmarks..."
    )
  endif()
endif()

# ============================================================================
# AGGREGATE TEST TARGETS
# ============================================================================

# Run all tests (unit + integration)
add_custom_target(run_tests
  COMMAND ${CMAKE_CTEST_COMMAND} --test-dir ${CMAKE_BINARY_DIR}
          --verbose --output-on-failure --label-regex "unit|integration"
  COMMENT "Running all unit and integration tests..."
)

# Run only unit tests
add_custom_target(run_unit_tests
  COMMAND ${CMAKE_CTEST_COMMAND} --test-dir ${CMAKE_BINARY_DIR}
          --verbose --output-on-failure --label-regex "^unit$"
  COMMENT "Running unit tests..."
)

# Run only integration tests
add_custom_target(run_integration_tests
  COMMAND ${CMAKE_CTEST_COMMAND} --test-dir ${CMAKE_BINARY_DIR}
          --verbose --output-on-failure --label-regex "^integration$"
  COMMENT "Running integration tests..."
)

# Run all benchmarks
add_custom_target(run_benchmarks
  COMMENT "Running all benchmarks..."
)

if(TARGET run_bench_crypto)
  add_dependencies(run_benchmarks run_bench_crypto)
endif()
if(TARGET run_bench_core)
  add_dependencies(run_benchmarks run_bench_core)
endif()
if(TARGET run_bench_consensus)
  add_dependencies(run_benchmarks run_bench_consensus)
endif()
if(TARGET run_bench_storage)
  add_dependencies(run_benchmarks run_bench_storage)
endif()

# Run tests with coverage (if enabled)
if(ENABLE_COVERAGE)
  add_custom_target(run_tests_with_coverage
    COMMAND ${CMAKE_CTEST_COMMAND} --test-dir ${CMAKE_BINARY_DIR}
            --verbose --output-on-failure
    COMMAND ninja -C ${CMAKE_BINARY_DIR} test_coverage
    COMMENT "Running tests with coverage analysis..."
  )
endif()

# ============================================================================
# REPORTING
# ============================================================================

message(STATUS "")
message(STATUS "QuantumVault Test Suite")
message(STATUS "========================")
message(STATUS "")
message(STATUS "Unit Tests:")
message(STATUS "  ctest --test-dir build -L unit                    # Run all unit tests")
message(STATUS "  ctest --test-dir build -R crypto                  # Run crypto tests")
message(STATUS "  ctest --test-dir build -R core                    # Run core tests")
message(STATUS "  ctest --test-dir build -R consensus               # Run consensus tests")
message(STATUS "")
message(STATUS "Integration Tests:")
message(STATUS "  ctest --test-dir build -L integration             # Run all integration tests")
message(STATUS "")
message(STATUS "Benchmarks:")
message(STATUS "  ninja -C build run_benchmarks                     # Run all benchmarks")
message(STATUS "  ./build/bin/bench_crypto --help                   # See benchmark options")
message(STATUS "")
message(STATUS "Coverage (if enabled with -DENABLE_COVERAGE=ON):")
message(STATUS "  ninja -C build run_tests_with_coverage            # Tests + coverage")
message(STATUS "  open build/coverage/index.html                    # View HTML report")
message(STATUS "")
message(STATUS "Documentation:")
message(STATUS "  See docs/TESTING_STRATEGY.md for comprehensive test strategy")
message(STATUS "  See tests/crypto/TEST_PLAN.md for crypto module test plan")
message(STATUS "  See docs/ADR/001-testing-framework.md for framework decisions")
message(STATUS "")
