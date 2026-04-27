# Network module CMakeLists.txt
# Builds qv_net_obj object library

file(GLOB_RECURSE NET_SOURCES "*.cpp")
file(GLOB_RECURSE NET_HEADERS "*.h")

add_library(qv_net_obj OBJECT
  ${NET_SOURCES}
)

target_include_directories(qv_net_obj PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/..
)

target_link_libraries(qv_net_obj PUBLIC
  spdlog::spdlog
  OpenSSL::SSL
  OpenSSL::Crypto
  Threads::Threads
)

set_target_properties(qv_net_obj PROPERTIES
  POSITION_INDEPENDENT_CODE ON
)
