cmake_minimum_required(VERSION 3.21)
project(omarecord VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Core Gui Quick Multimedia OpenGL Svg Test)
find_package(PkgConfig REQUIRED)
pkg_check_modules(LIBEVDEV REQUIRED IMPORTED_TARGET libevdev)

qt_standard_project_setup()

add_library(omarecord_core STATIC
    src/core/Project.cpp
    src/core/InputLog.cpp
    src/core/CursorPath.cpp
    src/core/MotionTrack.cpp
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

add_library(omarecord_render STATIC)
set_source_files_properties(
    src/ui/qml/Composition.qml
    src/ui/qml/Background.qml
    src/ui/qml/RoundedFrame.qml
    src/ui/qml/CursorOverlay.qml
    PROPERTIES QT_RESOURCE_ALIAS ""
)
set_source_files_properties(src/ui/qml/Composition.qml PROPERTIES QT_RESOURCE_ALIAS "Composition.qml")
set_source_files_properties(src/ui/qml/Background.qml PROPERTIES QT_RESOURCE_ALIAS "Background.qml")
set_source_files_properties(src/ui/qml/RoundedFrame.qml PROPERTIES QT_RESOURCE_ALIAS "RoundedFrame.qml")
set_source_files_properties(src/ui/qml/CursorOverlay.qml PROPERTIES QT_RESOURCE_ALIAS "CursorOverlay.qml")
qt_add_qml_module(omarecord_render
    URI Omarecord
    VERSION 1.0
    RESOURCE_PREFIX /qt/qml
    OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/Omarecord
    SOURCES
        src/render/FrameSource.h src/render/FrameSource.cpp
        src/render/FfmpegDecoder.h src/render/FfmpegDecoder.cpp
        src/render/Exporter.h src/render/Exporter.cpp
    QML_FILES
        src/ui/qml/Composition.qml
        src/ui/qml/Background.qml
        src/ui/qml/RoundedFrame.qml
        src/ui/qml/CursorOverlay.qml
)
qt_add_resources(omarecord_render "omarecord_assets"
    PREFIX "/omarecord"
    FILES assets/cursors/arrow.svg assets/cursors/pointer.svg
)
target_include_directories(omarecord_render PUBLIC src src/render)
target_link_libraries(omarecord_render PUBLIC omarecord_core Qt6::Quick Qt6::Multimedia Qt6::OpenGL)

qt_add_executable(omarecord src/main.cpp)
target_link_libraries(omarecord PRIVATE omarecord_render Qt6::Quick Qt6::Multimedia)

include(CTest)
if(BUILD_TESTING)
    foreach(test_name IN ITEMS zoomtimeline cursorpath cliptimeline project inputlog)
        qt_add_executable(test_${test_name} tests/test_${test_name}.cpp)
        target_link_libraries(test_${test_name} PRIVATE omarecord_core Qt6::Test)
        add_test(NAME ${test_name} COMMAND test_${test_name})
    endforeach()
    qt_add_executable(test_exporter_smoke tests/test_exporter_smoke.cpp)
    target_link_libraries(test_exporter_smoke PRIVATE omarecord_render Qt6::Test)
    add_test(NAME exporter_smoke COMMAND test_exporter_smoke)
endif()

install(TARGETS omarecord RUNTIME DESTINATION bin)
install(FILES pkg/omarecord.desktop DESTINATION share/applications)
