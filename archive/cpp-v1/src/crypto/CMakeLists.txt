# Crypto module CMakeLists.txt
# Builds qv_crypto_obj object library with PQC support

file(GLOB_RECURSE CRYPTO_SOURCES "*.cpp")
file(GLOB_RECURSE CRYPTO_HEADERS "*.h")

add_library(qv_crypto_obj OBJECT
  ${CRYPTO_SOURCES}
)

target_include_directories(qv_crypto_obj PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/..
)

target_link_libraries(qv_crypto_obj PUBLIC
  spdlog::spdlog
  PkgConfig::LIBOQS
  OpenSSL::Crypto
)

set_target_properties(qv_crypto_obj PROPERTIES
  POSITION_INDEPENDENT_CODE ON
)
