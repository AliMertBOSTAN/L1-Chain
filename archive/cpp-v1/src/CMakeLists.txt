# Main source directory CMakeLists.txt
# Builds the core library from module-specific object libraries

# Add subdirectories for each module
add_subdirectory(core)
add_subdirectory(crypto)
add_subdirectory(consensus)
add_subdirectory(net)
add_subdirectory(privacy)
add_subdirectory(da)
add_subdirectory(vm)
add_subdirectory(storage)
add_subdirectory(rpc)

# Create the main qv_core library from all module object libraries
add_library(qv_core STATIC
  $<TARGET_OBJECTS:qv_core_obj>
  $<TARGET_OBJECTS:qv_crypto_obj>
  $<TARGET_OBJECTS:qv_consensus_obj>
  $<TARGET_OBJECTS:qv_net_obj>
  $<TARGET_OBJECTS:qv_privacy_obj>
  $<TARGET_OBJECTS:qv_da_obj>
  $<TARGET_OBJECTS:qv_vm_obj>
  $<TARGET_OBJECTS:qv_storage_obj>
  $<TARGET_OBJECTS:qv_rpc_obj>
)

# Link dependencies to qv_core
target_link_libraries(qv_core PUBLIC
  Threads::Threads
  protobuf::libprotobuf
  spdlog::spdlog
  PkgConfig::LIBOQS
  OpenSSL::Crypto
  OpenSSL::SSL
)

# Link database library
if(LEVELDB_FOUND)
  target_link_libraries(qv_core PUBLIC PkgConfig::LEVELDB)
else()
  target_link_libraries(qv_core PUBLIC PkgConfig::ROCKSDB)
endif()

# Include directories
target_include_directories(qv_core PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_BINARY_DIR}
)

# Set library properties
set_target_properties(qv_core PROPERTIES
  VERSION ${PROJECT_VERSION}
  SOVERSION 1
  PUBLIC_HEADER "qv_core.h"
)

# Install library
install(TARGETS qv_core
  LIBRARY DESTINATION lib
  ARCHIVE DESTINATION lib
  RUNTIME DESTINATION bin
)

install(DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/
  DESTINATION include/qv_core
  FILES_MATCHING PATTERN "*.h"
)
