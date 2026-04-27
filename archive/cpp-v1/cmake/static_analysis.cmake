# QuantumVault Static Analysis Configuration
# Integrates clang-tidy, cppcheck, and include-what-you-use for comprehensive
# static analysis. Provides custom targets: 'lint', 'cppcheck', and 'iwyu'.

option(QV_ENABLE_CLANG_TIDY "Enable clang-tidy static analysis" OFF)
option(QV_ENABLE_CPPCHECK "Enable cppcheck static analysis" OFF)
option(QV_ENABLE_IWYU "Enable include-what-you-use analysis" OFF)

# Store list of source files for analysis targets
set(QV_SOURCE_DIRS
  ${CMAKE_SOURCE_DIR}/src
  ${CMAKE_SOURCE_DIR}/tools
  ${CMAKE_SOURCE_DIR}/tests
)

# Exclude directories and patterns from analysis
set(QV_EXCLUDE_PATTERNS
  "build/"
  ".git/"
  "third_party/"
  "external/"
  "cmake/"
)

# clang-tidy configuration
if(QV_ENABLE_CLANG_TIDY)
  find_program(CLANG_TIDY_EXE clang-tidy)
  if(NOT CLANG_TIDY_EXE)
    message(WARNING "clang-tidy not found. Install it to enable QV_ENABLE_CLANG_TIDY")
  else()
    message(STATUS "clang-tidy found: ${CLANG_TIDY_EXE}")

    # Get clang-tidy version
    execute_process(
      COMMAND ${CLANG_TIDY_EXE} --version
      OUTPUT_VARIABLE CLANG_TIDY_VERSION
      OUTPUT_STRIP_TRAILING_WHITESPACE
    )
    message(STATUS "${CLANG_TIDY_VERSION}")

    # Set clang-tidy for all targets
    set(CMAKE_CXX_CLANG_TIDY
      ${CLANG_TIDY_EXE}
      --fix
      --fix-errors
      --header-filter='.*qv.*'
    )
  endif()
endif()

# clang-tidy lint target (manual run on all sources)
find_program(CLANG_TIDY_EXE clang-tidy)
if(CLANG_TIDY_EXE)
  add_custom_target(lint
    COMMAND ${CMAKE_COMMAND} -E echo "Running clang-tidy on QuantumVault sources..."
    COMMAND ${CLANG_TIDY_EXE}
      --fix
      --fix-errors
      --header-filter='.*qv.*'
      -p=${CMAKE_BINARY_DIR}
      ${CMAKE_SOURCE_DIR}/src/**/*.cpp
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    COMMENT "Running clang-tidy static analysis on all sources"
    VERBATIM
  )
  message(STATUS "Custom target 'lint' available for clang-tidy analysis")
else()
  add_custom_target(lint
    COMMAND ${CMAKE_COMMAND} -E echo "Error: clang-tidy not found"
    COMMAND false
  )
  message(WARNING "clang-tidy not found - 'lint' target will fail")
endif()

# cppcheck configuration
if(QV_ENABLE_CPPCHECK)
  find_program(CPPCHECK_EXE cppcheck)
  if(NOT CPPCHECK_EXE)
    message(WARNING "cppcheck not found. Install it to enable QV_ENABLE_CPPCHECK")
  else()
    message(STATUS "cppcheck found: ${CPPCHECK_EXE}")

    # Get cppcheck version
    execute_process(
      COMMAND ${CPPCHECK_EXE} --version
      OUTPUT_VARIABLE CPPCHECK_VERSION
      OUTPUT_STRIP_TRAILING_WHITESPACE
    )
    message(STATUS "${CPPCHECK_VERSION}")

    # Set cppcheck for all targets
    set(CMAKE_CXX_CPPCHECK
      ${CPPCHECK_EXE}
      --enable=all
      --suppress=missingIncludeSystem
      --suppress=unmatchedSuppression
      --std=c++20
      --template=gcc
    )
  endif()
endif()

# cppcheck custom target
find_program(CPPCHECK_EXE cppcheck)
if(CPPCHECK_EXE)
  add_custom_target(cppcheck
    COMMAND ${CMAKE_COMMAND} -E echo "Running cppcheck on QuantumVault sources..."
    COMMAND ${CPPCHECK_EXE}
      --enable=all
      --suppress=missingIncludeSystem
      --suppress=unmatchedSuppression
      --std=c++20
      --template=gcc
      --project=${CMAKE_BINARY_DIR}/compile_commands.json
      --output-file=${CMAKE_BINARY_DIR}/cppcheck_report.txt
      ${QV_SOURCE_DIRS}
    COMMAND ${CMAKE_COMMAND} -E echo "cppcheck report written to: ${CMAKE_BINARY_DIR}/cppcheck_report.txt"
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    COMMENT "Running cppcheck static analysis"
    VERBATIM
  )
  message(STATUS "Custom target 'cppcheck' available for cppcheck analysis")
else()
  add_custom_target(cppcheck
    COMMAND ${CMAKE_COMMAND} -E echo "Error: cppcheck not found"
    COMMAND false
  )
  message(WARNING "cppcheck not found - 'cppcheck' target will fail")
endif()

# include-what-you-use configuration
if(QV_ENABLE_IWYU)
  find_program(IWYU_EXE include-what-you-use)
  if(NOT IWYU_EXE)
    message(WARNING "include-what-you-use not found. Install it to enable QV_ENABLE_IWYU")
  else()
    message(STATUS "include-what-you-use found: ${IWYU_EXE}")

    # Set iwyu for all targets
    set(CMAKE_CXX_INCLUDE_WHAT_YOU_USE
      ${IWYU_EXE}
      -Xiwyu
      --mapping_file=${CMAKE_SOURCE_DIR}/.iwyu_mapping
      -Xiwyu
      --max_line_length=120
    )
  endif()
endif()

# iwyu custom target
find_program(IWYU_EXE include-what-you-use)
if(IWYU_EXE)
  add_custom_target(iwyu
    COMMAND ${CMAKE_COMMAND} -E echo "Running include-what-you-use on QuantumVault sources..."
    COMMAND ${IWYU_EXE}
      -Xiwyu
      --mapping_file=${CMAKE_SOURCE_DIR}/.iwyu_mapping
      -Xiwyu
      --max_line_length=120
      -Xiwyu
      --check_also=*.hpp
      ${CMAKE_SOURCE_DIR}/src/**/*.cpp
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    COMMENT "Running include-what-you-use analysis"
    VERBATIM
  )
  message(STATUS "Custom target 'iwyu' available for include-what-you-use analysis")
else()
  add_custom_target(iwyu
    COMMAND ${CMAKE_COMMAND} -E echo "Error: include-what-you-use not found"
    COMMAND false
  )
  message(WARNING "include-what-you-use not found - 'iwyu' target will fail")
endif()

# Create a combined analysis target
add_custom_target(analyze
  DEPENDS lint cppcheck
  COMMENT "Running all available static analysis tools"
)

message(STATUS "Static analysis targets available:")
message(STATUS "  - lint     : Run clang-tidy")
message(STATUS "  - cppcheck : Run cppcheck")
message(STATUS "  - iwyu     : Run include-what-you-use")
message(STATUS "  - analyze  : Run all analysis tools")
