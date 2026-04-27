# RPC module CMakeLists.txt
# Builds qv_rpc_obj object library

file(GLOB_RECURSE RPC_SOURCES "*.cpp")
file(GLOB_RECURSE RPC_HEADERS "*.h")

add_library(qv_rpc_obj OBJECT
  ${RPC_SOURCES}
)

target_include_directories(qv_rpc_obj PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/..
)

target_link_libraries(qv_rpc_obj PUBLIC
  spdlog::spdlog
  protobuf::libprotobuf
)

set_target_properties(qv_rpc_obj PROPERTIES
  POSITION_INDEPENDENT_CODE ON
)
