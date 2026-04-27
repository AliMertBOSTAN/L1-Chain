# Tools CMakeLists.txt
# CLI tool targets for QuantumVault

# Function to add a tool executable
function(add_qv_tool tool_name tool_source)
  add_executable(${tool_name} ${tool_source})

  target_link_libraries(${tool_name} PRIVATE
    qv_core
    Threads::Threads
    spdlog::spdlog
  )

  target_include_directories(${tool_name} PRIVATE
    ${CMAKE_SOURCE_DIR}/src
    ${CMAKE_CURRENT_SOURCE_DIR}
  )

  set_target_properties(${tool_name} PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY ${CMAKE_RUNTIME_OUTPUT_DIRECTORY}
  )

  install(TARGETS ${tool_name}
    RUNTIME DESTINATION bin
  )
endfunction()

# Add individual tools - only if source files exist
if(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/qv-node.cpp")
  add_qv_tool(qv-node qv-node.cpp)
endif()

if(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/qv-wallet.cpp")
  add_qv_tool(qv-wallet qv-wallet.cpp)
endif()

if(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/qv-miner.cpp")
  add_qv_tool(qv-miner qv-miner.cpp)
endif()

if(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/qv-dslc.cpp")
  add_qv_tool(qv-dslc qv-dslc.cpp)
endif()

message(STATUS "Tools configured for QuantumVault")
