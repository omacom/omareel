# The golden-fixture oracle: a standalone project (not part of omareel's build) that compiles
# the C++ core of the tag rust-migration-base and records what it produces over
# tests/golden/corpus. tools/oracle/generate-golden.sh is the way to run it.
#
#   cmake -S tools/oracle -B build-oracle -DOMAREEL_BASE=<directory holding the tag's src/>
cmake_minimum_required(VERSION 3.21)
project(omareel_oracle LANGUAGES CXX)

set(OMAREEL_BASE "" CACHE PATH "Directory that contains the src/ tree of rust-migration-base")
if(NOT OMAREEL_BASE OR NOT EXISTS "${OMAREEL_BASE}/src/core/Project.cpp")
    message(FATAL_ERROR "Set OMAREEL_BASE to a directory holding the tag's src/ (got '${OMAREEL_BASE}')")
endif()

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui)

file(GLOB ORACLE_BASE_SOURCES CONFIGURE_DEPENDS "${OMAREEL_BASE}/src/core/*.cpp")
# Headers next to the sources are listed so AUTOMOC sees Theme.h's Q_OBJECT.
file(GLOB ORACLE_BASE_HEADERS CONFIGURE_DEPENDS "${OMAREEL_BASE}/src/core/*.h")
# The one file of src/render the oracle runs: it needs only QtCore (QProcess for ffprobe).
list(APPEND ORACLE_BASE_SOURCES "${OMAREEL_BASE}/src/render/MediaProbe.cpp")
list(APPEND ORACLE_BASE_HEADERS "${OMAREEL_BASE}/src/render/MediaProbe.h")
file(GLOB ORACLE_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/cases/*.cpp")

add_executable(omareel-oracle ${ORACLE_SOURCES} ${ORACLE_BASE_SOURCES} ${ORACLE_BASE_HEADERS})
target_include_directories(omareel-oracle PRIVATE "${OMAREEL_BASE}/src" "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(omareel-oracle PRIVATE Qt6::Core Qt6::Gui)
# Float parity: no fused multiply-add and no fast-math, so every expression rounds the way the
# Rust port's will.
target_compile_options(omareel-oracle PRIVATE -ffp-contract=off -fno-fast-math)
