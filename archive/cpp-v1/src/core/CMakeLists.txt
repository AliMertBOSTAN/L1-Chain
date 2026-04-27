# Core module CMakeLists.txt
# Builds qv_core_obj object library

file(GLOB_RECURSE CORE_SOURCES "*.cpp")
file(GLOB_RECURSE CORE_HEADERS "*.h")

add_library(qv_core_obj OBJECT
  ${CORE_SOURCES}
)

target_include_directories(qv_core_obj PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/..
)

target_link_libraries(qv_core_obj PUBLIC
  spdlog::spdlog
)

set_target_properties(qv_core_obj PROPERTIES
  POSITION_INDEPENDENT_CODE ON
)
