# QuantumVault Sanitizers Configuration
# Provides CMake options for AddressSanitizer, UndefinedBehaviorSanitizer,
# ThreadSanitizer, and MemorySanitizer with proper compiler and linker flags.

option(QV_ENABLE_ASAN "Enable AddressSanitizer (detects memory errors)" OFF)
option(QV_ENABLE_UBSAN "Enable UndefinedBehaviorSanitizer (detects undefined behavior)" OFF)
option(QV_ENABLE_TSAN "Enable ThreadSanitizer (detects data races)" OFF)
option(QV_ENABLE_MSAN "Enable MemorySanitizer (detects uninitialized memory)" OFF)

# Validate sanitizer combinations
if(QV_ENABLE_ASAN AND QV_ENABLE_TSAN)
  message(WARNING "AddressSanitizer and ThreadSanitizer are mutually exclusive. "
                  "Both are enabled - this may cause issues. Consider disabling one.")
endif()

if(QV_ENABLE_MSAN AND QV_ENABLE_ASAN)
  message(WARNING "MemorySanitizer and AddressSanitizer are mutually exclusive. "
                  "Both are enabled - this may cause issues. Consider disabling one.")
endif()

if(QV_ENABLE_MSAN AND QV_ENABLE_TSAN)
  message(WARNING "MemorySanitizer and ThreadSanitizer are mutually exclusive. "
                  "Both are enabled - this may cause issues. Consider disabling one.")
endif()

# Sanitizer flags
set(SANITIZER_FLAGS "")
set(SANITIZER_LINKER_FLAGS "")

# AddressSanitizer (ASAN)
if(QV_ENABLE_ASAN)
  message(STATUS "AddressSanitizer enabled")
  list(APPEND SANITIZER_FLAGS "-fsanitize=address")
  list(APPEND SANITIZER_LINKER_FLAGS "-fsanitize=address")

  # Enable leak detection (not available on all platforms)
  if(NOT CMAKE_CXX_COMPILER_ID MATCHES "MSVC")
    list(APPEND SANITIZER_FLAGS "-fsanitize=leak")
    list(APPEND SANITIZER_LINKER_FLAGS "-fsanitize=leak")
  endif()
endif()

# UndefinedBehaviorSanitizer (UBSAN)
if(QV_ENABLE_UBSAN)
  message(STATUS "UndefinedBehaviorSanitizer enabled")
  list(APPEND SANITIZER_FLAGS
    "-fsanitize=undefined"
    "-fno-sanitize-recover=undefined"
  )
  list(APPEND SANITIZER_LINKER_FLAGS "-fsanitize=undefined")

  # Enhanced checks for crypto code
  list(APPEND SANITIZER_FLAGS
    "-fsanitize=signed-integer-overflow"
    "-fsanitize=shift"
    "-fsanitize=alignment"
  )
endif()

# ThreadSanitizer (TSAN)
if(QV_ENABLE_TSAN)
  message(STATUS "ThreadSanitizer enabled")
  list(APPEND SANITIZER_FLAGS "-fsanitize=thread")
  list(APPEND SANITIZER_LINKER_FLAGS "-fsanitize=thread")
endif()

# MemorySanitizer (MSAN)
if(QV_ENABLE_MSAN)
  message(STATUS "MemorySanitizer enabled")
  list(APPEND SANITIZER_FLAGS
    "-fsanitize=memory"
    "-fsanitize-memory-track-origins=2"
  )
  list(APPEND SANITIZER_LINKER_FLAGS "-fsanitize=memory")
endif()

# Only apply sanitizer flags if at least one is enabled
if(SANITIZER_FLAGS)
  # Add sanitizer flags to compile and linker
  add_compile_options(${SANITIZER_FLAGS})
  add_link_options(${SANITIZER_LINKER_FLAGS})

  # Set common sanitizer options
  if(NOT CMAKE_CXX_COMPILER_ID MATCHES "MSVC")
    # Improve stack traces
    add_compile_options(-fno-omit-frame-pointer)

    # Better symbolization
    if(CMAKE_BUILD_TYPE STREQUAL "Debug")
      add_compile_options(-g)
    endif()
  endif()

  # Environment variables for sanitizer behavior
  set(SANITIZER_OPTIONS "")

  if(QV_ENABLE_ASAN)
    list(APPEND SANITIZER_OPTIONS
      "ASAN_OPTIONS=verbosity=1:halt_on_error=1:detect_leaks=1"
    )
  endif()

  if(QV_ENABLE_UBSAN)
    list(APPEND SANITIZER_OPTIONS
      "UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1"
    )
  endif()

  if(QV_ENABLE_TSAN)
    list(APPEND SANITIZER_OPTIONS
      "TSAN_OPTIONS=verbosity=1:halt_on_error=1"
    )
  endif()

  if(QV_ENABLE_MSAN)
    list(APPEND SANITIZER_OPTIONS
      "MSAN_OPTIONS=verbosity=1:halt_on_error=1"
    )
  endif()

  # Create a property to store sanitizer options
  set(QV_SANITIZER_OPTIONS "${SANITIZER_OPTIONS}" CACHE STRING
    "Sanitizer environment options" INTERNAL)

  message(STATUS "Sanitizer configuration: ${SANITIZER_FLAGS}")
endif()
