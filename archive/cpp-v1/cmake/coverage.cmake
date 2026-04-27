# QuantumVault Code Coverage Configuration
# Provides code coverage support using gcov/lcov (GCC) or llvm-cov (Clang).
# Generates HTML coverage reports and custom 'coverage' target.

option(QV_ENABLE_COVERAGE "Enable code coverage reporting" OFF)

if(QV_ENABLE_COVERAGE)
  message(STATUS "Code coverage enabled")

  # Detect compiler and set appropriate flags
  if(CMAKE_CXX_COMPILER_ID MATCHES "GNU")
    set(COVERAGE_COMPILER "gcc")
    set(COVERAGE_FLAGS "--coverage" "-fprofile-arcs" "-ftest-coverage")
    set(COVERAGE_LINKER_FLAGS "--coverage")
  elseif(CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    set(COVERAGE_COMPILER "clang")
    set(COVERAGE_FLAGS "-fprofile-instr-generate" "-fcoverage-mapping")
    set(COVERAGE_LINKER_FLAGS "-fprofile-instr-generate" "-fcoverage-mapping")
  else()
    message(WARNING "Code coverage is only supported with GCC or Clang")
    return()
  endif()

  message(STATUS "Code coverage compiler: ${COVERAGE_COMPILER}")

  # Add coverage flags to compiler
  add_compile_options(${COVERAGE_FLAGS})
  add_link_options(${COVERAGE_LINKER_FLAGS})

  # Ensure debug symbols are available
  if(NOT CMAKE_BUILD_TYPE STREQUAL "Debug")
    message(WARNING "Code coverage works best with Debug build type")
  endif()

  # Find required tools
  find_program(LCOV lcov)
  find_program(GENHTML genhtml)
  find_program(LLVM_COV llvm-cov)
  find_program(LLVM_PROFDATA llvm-profdata)

  # Create custom target for coverage report
  if(COVERAGE_COMPILER STREQUAL "gcc" AND LCOV AND GENHTML)
    add_custom_target(coverage
      COMMAND ${CMAKE_COMMAND} --build ${CMAKE_BINARY_DIR} --target test
      COMMAND ${LCOV}
        --capture
        --directory ${CMAKE_BINARY_DIR}
        --output-file ${CMAKE_BINARY_DIR}/coverage.info
        --no-external
        --base-directory ${CMAKE_SOURCE_DIR}
      COMMAND ${LCOV}
        --remove ${CMAKE_BINARY_DIR}/coverage.info
        '*/tests/*'
        '*/test_*.cpp'
        '*/gtest/*'
        '*/benchmark/*'
        '*/third_party/*'
        '*/external/*'
        '/usr/*'
        --output-file ${CMAKE_BINARY_DIR}/coverage_filtered.info
      COMMAND ${GENHTML}
        --output-directory ${CMAKE_BINARY_DIR}/coverage_report
        --title "QuantumVault Code Coverage Report"
        --legend
        --quiet
        ${CMAKE_BINARY_DIR}/coverage_filtered.info
      COMMAND ${CMAKE_COMMAND} -E echo "Coverage report generated: ${CMAKE_BINARY_DIR}/coverage_report/index.html"
      WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
      COMMENT "Generating code coverage report using lcov..."
      VERBATIM
    )
    message(STATUS "Coverage target: 'coverage' (uses lcov/genhtml)")

  elseif(COVERAGE_COMPILER STREQUAL "clang" AND LLVM_COV AND LLVM_PROFDATA)
    # Clang coverage with llvm-cov
    add_custom_target(coverage
      COMMAND ${CMAKE_COMMAND} --build ${CMAKE_BINARY_DIR} --target test
      COMMAND ${LLVM_PROFDATA} merge
        -o ${CMAKE_BINARY_DIR}/default.profdata
        ${CMAKE_BINARY_DIR}/default.profraw
      COMMAND ${LLVM_COV} show
        -instr-profile=${CMAKE_BINARY_DIR}/default.profdata
        -format=html
        -output-dir=${CMAKE_BINARY_DIR}/coverage_report
        -ignore-filename-regex='(tests|test_|gtest|benchmark|third_party|external)'
        ${CMAKE_BINARY_DIR}/bin/*
      COMMAND ${CMAKE_COMMAND} -E echo "Coverage report generated: ${CMAKE_BINARY_DIR}/coverage_report/index.html"
      WORKING_DIRECTORY ${CMAKE_BINARY_DIR}
      COMMENT "Generating code coverage report using llvm-cov..."
      VERBATIM
    )
    message(STATUS "Coverage target: 'coverage' (uses llvm-cov)")

  else()
    message(WARNING "Coverage tools not found. Install lcov (GCC) or llvm-tools (Clang)")
    add_custom_target(coverage
      COMMAND ${CMAKE_COMMAND} -E echo "Error: Coverage tools not found"
      COMMAND false
    )
  endif()

endif()
