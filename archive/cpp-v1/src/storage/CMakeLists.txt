# Storage module CMakeLists.txt
# Builds qv_storage_obj object library with database support

file(GLOB_RECURSE STORAGE_SOURCES "*.cpp")
file(GLOB_RECURSE STORAGE_HEADERS "*.h")

add_library(qv_storage_obj OBJECT
  ${STORAGE_SOURCES}
)

target_include_directories(qv_storage_obj PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/..
)

target_link_libraries(qv_storage_obj PUBLIC
  spdlog::spdlog
)

# Link appropriate database library
if(LEVELDB_FOUND)
  target_link_libraries(qv_storage_obj PUBLIC PkgConfig::LEVELDB)
else()
  target_link_libraries(qv_storage_obj PUBLIC PkgConfig::ROCKSDB)
endif()

set_target_properties(qv_storage_obj PROPERTIES
  POSITION_INDEPENDENT_CODE ON
)
