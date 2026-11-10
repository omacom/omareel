cmake_minimum_required(VERSION 3.25)
project(omareel_capture_exclude VERSION 0.1.0 LANGUAGES CXX)
find_package(PkgConfig REQUIRED)
pkg_check_modules(HYPRLAND REQUIRED hyprland)
pkg_check_modules(PLUGIN_GL REQUIRED IMPORTED_TARGET pixman-1 libdrm)
# Do not allow /usr/local/include's stale hyprland tree to shadow the package.
set(OMAREEL_HYPRLAND_HEADERS /usr/include/hyprland CACHE PATH "Matching packaged Hyprland headers")
# More Hyprland versions to build the plugin for, each a prefix holding include/hyprland and the
# include/aquamarine, include/hyprutils and include/hyprgraphics it was built with (unpacked from
# those packages). Each build goes to hyprland-<version>/ and the app loads the one whose commit
# matches the running Hyprland. Used to cover Omarchy's stable channel while it is a Hyprland
# minor behind edge.
set(OMAREEL_EXTRA_HYPRLAND_SDKS "" CACHE STRING "Prefixes of other Hyprland versions' headers")

# Builds the plugin against `headers` (a Hyprland include/hyprland tree), with `prefix_include`
# searched ahead of the system for its dependencies' headers when set, into `output_dir`, and
# installs it to lib/omareel/`install_subdir`.
function(omareel_capture_exclude target headers prefix_include output_dir install_subdir)
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${headers}/src/version.h")
    set(private_include "${CMAKE_CURRENT_BINARY_DIR}/${target}-include")
    file(MAKE_DIRECTORY "${private_include}")
    file(CREATE_LINK "${headers}" "${private_include}/hyprland" SYMBOLIC)
    file(STRINGS "${headers}/src/version.h" hash_line REGEX "^#define GIT_COMMIT_HASH ")
    string(REGEX MATCH "\"([a-f0-9]+)\"" hash_match "${hash_line}")
    if(NOT CMAKE_MATCH_1)
        message(FATAL_ERROR "Cannot determine the Hyprland commit of ${headers}")
    endif()
    set(hash "${CMAKE_MATCH_1}")
    file(STRINGS "${headers}/src/version.h" tag_line REGEX "^#define GIT_TAG ")
    string(REGEX MATCH "\"v?([0-9]+)\\.([0-9]+)" tag_match "${tag_line}")
    if(NOT CMAKE_MATCH_2)
        message(FATAL_ERROR "Cannot determine the Hyprland version of ${headers}")
    endif()
    set(minor "${CMAKE_MATCH_2}")
    file(MAKE_DIRECTORY "${output_dir}")
    file(WRITE "${output_dir}/omareel-capture-exclude.so.hash" "${hash}\n")
    add_library(${target} MODULE CaptureExclude.cpp)
    set_target_properties(${target} PROPERTIES PREFIX "" OUTPUT_NAME omareel-capture-exclude
        LIBRARY_OUTPUT_DIRECTORY "${output_dir}" AUTOMOC OFF CXX_STANDARD 26 CXX_STANDARD_REQUIRED ON)
    target_include_directories(${target} BEFORE PRIVATE
        "${private_include}" "${headers}" "${headers}/src" "${headers}/protocols" ${prefix_include})
    target_compile_definitions(${target} PRIVATE WLR_USE_UNSTABLE OMAREEL_HYPRLAND_MINOR=${minor})
    # Hyprland's assertion macros embed header paths; map our private include
    # symlink back to the packaged tree so no build directory leaks into the .so.
    target_compile_options(${target} PRIVATE
        "-ffile-prefix-map=${private_include}/hyprland=/usr/include/hyprland"
        "-ffile-prefix-map=${CMAKE_CURRENT_SOURCE_DIR}=plugin")
    if(prefix_include)
        target_compile_options(${target} PRIVATE "-ffile-prefix-map=${prefix_include}=/usr/include")
    endif()
    target_link_libraries(${target} PRIVATE PkgConfig::PLUGIN_GL)
    install(TARGETS ${target} LIBRARY DESTINATION "lib/omareel/${install_subdir}")
    install(FILES "${output_dir}/omareel-capture-exclude.so.hash" DESTINATION "lib/omareel/${install_subdir}")
    message(STATUS "Capture exclusion plugin for Hyprland 0.${minor} (${hash}) in ${output_dir}")
endfunction()

omareel_capture_exclude(omareel-capture-exclude "${OMAREEL_HYPRLAND_HEADERS}" "" "${CMAKE_CURRENT_BINARY_DIR}" "")
foreach(sdk IN LISTS OMAREEL_EXTRA_HYPRLAND_SDKS)
    file(STRINGS "${sdk}/include/hyprland/src/version.h" sdk_tag REGEX "^#define GIT_TAG ")
    string(REGEX MATCH "\"v?([0-9.]+)\"" sdk_match "${sdk_tag}")
    if(NOT CMAKE_MATCH_1)
        message(FATAL_ERROR "Cannot determine the Hyprland version of ${sdk}")
    endif()
    set(version "${CMAKE_MATCH_1}")
    string(REPLACE "." "_" suffix "${version}")
    omareel_capture_exclude(omareel-capture-exclude-${suffix} "${sdk}/include/hyprland" "${sdk}/include"
        "${CMAKE_CURRENT_BINARY_DIR}/hyprland-${version}" "hyprland-${version}")
endforeach()
