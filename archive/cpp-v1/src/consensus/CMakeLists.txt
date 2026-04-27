# Consensus module CMakeLists.txt
# Builds qv_consensus_obj object library

file(GLOB_RECURSE CONSENSUS_SOURCES "*.cpp")
file(GLOB_RECURSE CONSENSUS_HEADERS "*.h")

add_library(qv_consensus_obj OBJECT
  ${CONSENSUS_SOURCES}
)

target_include_directories(qv_consensus_obj PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/..
)

target_link_libraries(qv_consensus_obj PUBLIC
  spdlog::spdlog
)

set_target_properties(qv_consensus_obj PROPERTIES
  POSITION_INDEPENDENT_CODE ON
)
