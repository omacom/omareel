# The golden-fixture oracle: a standalone project (not part of omareel's build) that compiles
# the C++ core of the tag rust-migration-base and records what it produces over
# tests/golden/corpus. tools/oracle/generate-golden.sh is the way to run it.
#
#   cmake -S tools/oracle -B build-oracle -DOMAREEL_BASE=<directory holding the tag's src/>
cmake_minimum_required(VERSION 3.21)
project(omareel_oracle LANGUAGES C CXX)

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

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui Multimedia MultimediaPrivate)
# The recording side (src/record) links what the app links: libevdev, libwayland-client and
# libturbojpeg, and the capture protocols generated as the app's CMakeLists.txt does (the
# foreign-toplevel list only because the image-capture-source code refers to its interface).
find_package(PkgConfig REQUIRED)
pkg_check_modules(TURBOJPEG REQUIRED IMPORTED_TARGET libturbojpeg)
pkg_check_modules(LIBEVDEV REQUIRED IMPORTED_TARGET libevdev)
pkg_check_modules(WAYLAND_CLIENT REQUIRED IMPORTED_TARGET wayland-client)
find_program(WAYLAND_SCANNER REQUIRED NAMES wayland-scanner)
set(ORACLE_GENERATED "${CMAKE_CURRENT_BINARY_DIR}/generated")
set(ORACLE_WAYLAND_SOURCES "")
foreach(protocol IN ITEMS ext-image-capture-source-v1 ext-image-copy-capture-v1 ext-foreign-toplevel-list-v1)
    string(REGEX REPLACE "-v1$" "" protocol_directory "${protocol}")
    set(protocol_xml "/usr/share/wayland-protocols/staging/${protocol_directory}/${protocol}.xml")
    set(protocol_header "${ORACLE_GENERATED}/${protocol}-client-protocol.h")
    set(protocol_code "${ORACLE_GENERATED}/${protocol}-protocol.c")
    add_custom_command(OUTPUT "${protocol_header}" "${protocol_code}"
        COMMAND "${CMAKE_COMMAND}" -E make_directory "${ORACLE_GENERATED}"
        COMMAND "${WAYLAND_SCANNER}" client-header "${protocol_xml}" "${protocol_header}"
        COMMAND "${WAYLAND_SCANNER}" private-code "${protocol_xml}" "${protocol_code}"
        DEPENDS "${protocol_xml}" VERBATIM)
    list(APPEND ORACLE_WAYLAND_SOURCES "${protocol_header}" "${protocol_code}")
endforeach()

file(GLOB ORACLE_BASE_SOURCES CONFIGURE_DEPENDS "${OMAREEL_BASE}/src/core/*.cpp")
# Headers next to the sources are listed so AUTOMOC sees Theme.h's Q_OBJECT.
file(GLOB ORACLE_BASE_HEADERS CONFIGURE_DEPENDS "${OMAREEL_BASE}/src/core/*.h")
# The one file of src/render the oracle runs: it needs only QtCore (QProcess for ffprobe).
list(APPEND ORACLE_BASE_SOURCES "${OMAREEL_BASE}/src/render/MediaProbe.cpp")
list(APPEND ORACLE_BASE_HEADERS "${OMAREEL_BASE}/src/render/MediaProbe.h")
# The recording side. Recorder.cpp, RegionPicker.cpp, ScreenCapture.cpp and CameraCapture.cpp are
# not listed: cases/recorder.cpp, regionpicker.cpp, screencapture.cpp and camera.cpp #include them
# whole, so their file-static helpers can be called as the tag has them, and each is compiled
# exactly once that way. The headers with Q_OBJECT are listed for AUTOMOC.
foreach(file IN ITEMS record/CursorSampler.cpp record/EvdevListener.cpp record/CaptureExclusion.cpp
                      ui/MicrophoneMeter.cpp)
    list(APPEND ORACLE_BASE_SOURCES "${OMAREEL_BASE}/src/${file}")
endforeach()
foreach(file IN ITEMS record/CameraCapture.h ui/MicrophoneMeter.h)
    list(APPEND ORACLE_BASE_HEADERS "${OMAREEL_BASE}/src/${file}")
endforeach()
file(GLOB ORACLE_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/cases/*.cpp")

add_executable(omareel-oracle ${ORACLE_SOURCES} ${ORACLE_BASE_SOURCES} ${ORACLE_BASE_HEADERS}
               ${ORACLE_WAYLAND_SOURCES})
target_include_directories(omareel-oracle PRIVATE "${OMAREEL_BASE}/src" "${CMAKE_CURRENT_SOURCE_DIR}"
                           "${ORACLE_GENERATED}")
target_link_libraries(omareel-oracle PRIVATE Qt6::Core Qt6::Gui Qt6::Multimedia Qt6::MultimediaPrivate
                      PkgConfig::LIBEVDEV PkgConfig::WAYLAND_CLIENT PkgConfig::TURBOJPEG)
# Float parity: no fused multiply-add and no fast-math, so every expression rounds the way the
# Rust port's will.
target_compile_options(omareel-oracle PRIVATE $<$<COMPILE_LANGUAGE:CXX>:-ffp-contract=off -fno-fast-math>)
