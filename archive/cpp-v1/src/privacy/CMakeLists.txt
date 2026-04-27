# Privacy module CMakeLists.txt
# Builds qv_privacy_obj object library

file(GLOB_RECURSE PRIVACY_SOURCES "*.cpp")
file(GLOB_RECURSE PRIVACY_HEADERS "*.h")

add_library(qv_privacy_obj OBJECT
  ${PRIVACY_SOURCES}
)

target_include_directories(qv_privacy_obj PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/..
)

target_link_libraries(qv_privacy_obj PUBLIC
  spdlog::spdlog
  PkgConfig::LIBOQS
)

set_target_properties(qv_privacy_obj PROPERTIES
  POSITION_INDEPENDENT_CODE ON
)
