# Data Availability module CMakeLists.txt
# Builds qv_da_obj object library

file(GLOB_RECURSE DA_SOURCES "*.cpp")
file(GLOB_RECURSE DA_HEADERS "*.h")

add_library(qv_da_obj OBJECT
  ${DA_SOURCES}
)

target_include_directories(qv_da_obj PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/..
)

target_link_libraries(qv_da_obj PUBLIC
  spdlog::spdlog
)

set_target_properties(qv_da_obj PROPERTIES
  POSITION_INDEPENDENT_CODE ON
)
