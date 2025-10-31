cmake_minimum_required(VERSION 3.20)
cmake_policy(SET CMP0144 NEW)
project(openzl_arrow_parser CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Find zstd dependency required by OpenZL
find_package(zstd REQUIRED)
find_package(openzl REQUIRED)

# OTAP parser library
add_library(otap_parser STATIC
    otap_proto.cpp
    otap_parser.cpp
)

target_link_libraries(otap_parser PUBLIC
    OpenZL::openzl
)

target_include_directories(otap_parser PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}
)

# Add paths to OpenZL training tools
set(OPENZL_BUILD_DIR /home/danny/Repos/openzl/build-install)
link_directories(
    ${OPENZL_BUILD_DIR}/tools/training
    ${OPENZL_BUILD_DIR}/tools/io
    ${OPENZL_BUILD_DIR}/tools/logger
    ${OPENZL_BUILD_DIR}/custom_parsers
)

# Training executable (with ACE support)
add_executable(train_otap
    train_otap.cpp
)

target_link_libraries(train_otap PRIVATE
    otap_parser
    tools_training        # Training API
    tools_io              # Input file handling
    logger                # Logging
    OpenZL::openzl_cpp
    OpenZL::openzl
    stdc++
)

target_include_directories(train_otap PRIVATE
    /home/danny/Repos/openzl              # Root for custom_parsers
)

# Testing executable
add_executable(test_otap
    test_otap.cpp
)

target_link_libraries(test_otap PRIVATE
    otap_parser
    OpenZL::openzl
    OpenZL::openzl_cpp
    stdc++
)

# Original simple test
add_executable(simple_compress_test
    simple_compress_test.cpp
)

target_link_libraries(simple_compress_test PRIVATE
    OpenZL::openzl
    stdc++
)

install(TARGETS train_otap test_otap simple_compress_test
    RUNTIME DESTINATION bin
)
