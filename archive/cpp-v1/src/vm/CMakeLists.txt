# Virtual Machine module CMakeLists.txt
# Builds qv_vm_obj object library

file(GLOB_RECURSE VM_SOURCES "*.cpp")
file(GLOB_RECURSE VM_HEADERS "*.h")

add_library(qv_vm_obj OBJECT
  ${VM_SOURCES}
)

target_include_directories(qv_vm_obj PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/..
)

target_link_libraries(qv_vm_obj PUBLIC
  spdlog::spdlog
)

set_target_properties(qv_vm_obj PROPERTIES
  POSITION_INDEPENDENT_CODE ON
)
