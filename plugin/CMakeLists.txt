cmake_minimum_required(VERSION 3.25)
project(omareel_capture_exclude VERSION 0.1.0 LANGUAGES CXX)
find_package(PkgConfig REQUIRED)
pkg_check_modules(HYPRLAND REQUIRED hyprland)
pkg_check_modules(PLUGIN_GL REQUIRED IMPORTED_TARGET pixman-1 libdrm)
# Do not allow /usr/local/include's stale hyprland tree to shadow the package.
set(OMAREEL_HYPRLAND_HEADERS /usr/include/hyprland CACHE PATH "Matching packaged Hyprland headers")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${OMAREEL_HYPRLAND_HEADERS}/src/version.h")
file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/include")
file(CREATE_LINK "${OMAREEL_HYPRLAND_HEADERS}" "${CMAKE_CURRENT_BINARY_DIR}/include/hyprland" SYMBOLIC)
file(STRINGS "${OMAREEL_HYPRLAND_HEADERS}/src/version.h" hash_line REGEX "^#define GIT_COMMIT_HASH ")
string(REGEX MATCH "\"([a-f0-9]+)\"" hash_match "${hash_line}")
if(NOT CMAKE_MATCH_1)
    message(FATAL_ERROR "Cannot determine packaged Hyprland commit")
endif()
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/omareel-capture-exclude.so.hash" "${CMAKE_MATCH_1}\n")
add_library(omareel-capture-exclude MODULE CaptureExclude.cpp)
set_target_properties(omareel-capture-exclude PROPERTIES PREFIX "" AUTOMOC OFF CXX_STANDARD 26 CXX_STANDARD_REQUIRED ON)
target_include_directories(omareel-capture-exclude BEFORE PRIVATE
    "${CMAKE_CURRENT_BINARY_DIR}/include" "${OMAREEL_HYPRLAND_HEADERS}"
    "${OMAREEL_HYPRLAND_HEADERS}/src" "${OMAREEL_HYPRLAND_HEADERS}/protocols")
target_compile_definitions(omareel-capture-exclude PRIVATE WLR_USE_UNSTABLE)
# Hyprland's assertion macros embed header paths; map our private include
# symlink back to the packaged tree so no build directory leaks into the .so.
target_compile_options(omareel-capture-exclude PRIVATE
    "-ffile-prefix-map=${CMAKE_CURRENT_BINARY_DIR}/include/hyprland=${OMAREEL_HYPRLAND_HEADERS}"
    "-ffile-prefix-map=${CMAKE_CURRENT_SOURCE_DIR}=plugin")
target_link_libraries(omareel-capture-exclude PRIVATE PkgConfig::PLUGIN_GL)
install(TARGETS omareel-capture-exclude LIBRARY DESTINATION lib/omareel)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/omareel-capture-exclude.so.hash" DESTINATION lib/omareel)
