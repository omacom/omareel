cmake_minimum_required(VERSION 3.21)
project(omarecord VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Core Gui Quick Multimedia Svg Test)
find_package(PkgConfig REQUIRED)
pkg_check_modules(LIBEVDEV REQUIRED IMPORTED_TARGET libevdev)

qt_standard_project_setup()

add_library(omarecord_core STATIC
    src/core/Project.cpp
    src/core/InputLog.cpp
    src/core/CursorPath.cpp
    src/core/ZoomTimeline.cpp
    src/core/ClipTimeline.cpp
    src/core/Theme.cpp
    src/record/CursorSampler.cpp
    src/record/EvdevListener.cpp
    src/record/RegionPicker.cpp
    src/record/Recorder.cpp
)
target_include_directories(omarecord_core PUBLIC src)
target_link_libraries(omarecord_core
    PUBLIC Qt6::Core Qt6::Gui Qt6::Quick Qt6::Multimedia Qt6::Svg
    PRIVATE PkgConfig::LIBEVDEV
)

qt_add_executable(omarecord src/main.cpp)
target_link_libraries(omarecord PRIVATE omarecord_core Qt6::Quick Qt6::Multimedia)

include(CTest)
if(BUILD_TESTING)
    foreach(test_name IN ITEMS zoomtimeline cursorpath cliptimeline project inputlog)
        qt_add_executable(test_${test_name} tests/test_${test_name}.cpp)
        target_link_libraries(test_${test_name} PRIVATE omarecord_core Qt6::Test)
        add_test(NAME ${test_name} COMMAND test_${test_name})
    endforeach()
endif()

install(TARGETS omarecord RUNTIME DESTINATION bin)
install(FILES pkg/omarecord.desktop DESTINATION share/applications)
