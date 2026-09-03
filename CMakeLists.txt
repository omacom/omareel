cmake_minimum_required(VERSION 3.21)
project(omarecord VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Concurrent Gui Quick QuickControls2 Multimedia OpenGL Svg Test)
find_package(LayerShellQt REQUIRED)
find_package(PkgConfig REQUIRED)
pkg_check_modules(LIBEVDEV REQUIRED IMPORTED_TARGET libevdev)

qt_standard_project_setup()
qt_policy(SET QTP0004 NEW)

add_library(omarecord_core STATIC
    src/core/Project.cpp
    src/core/InputLog.cpp
    src/core/CursorPath.cpp
    src/core/MotionTrack.cpp
    src/core/KeystrokeTrack.cpp
    src/core/ZoomTimeline.cpp
    src/core/ClipTimeline.cpp
    src/core/CameraTimeline.cpp
    src/core/Theme.cpp
    src/core/OmarchyPaths.cpp
    src/core/RecordingMetadata.cpp
    src/core/RecordingPreferences.cpp
    src/record/CursorSampler.cpp
    src/record/CameraCapture.cpp
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
qt_add_qml_module(omarecord_ui STATIC
    URI Omarecord.Ui
    VERSION 1.0
    RESOURCE_PREFIX /qt/qml
    OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/Omarecord/Ui
    QML_FILES
        src/ui/qml/ui/OmButton.qml
        src/ui/qml/ui/OmButtonGroup.qml
        src/ui/qml/ui/OmToggle.qml
        src/ui/qml/ui/OmSlider.qml
        src/ui/qml/ui/OmSectionHeader.qml
        src/ui/qml/ui/OmHero.qml
        src/ui/qml/ui/OmSeparator.qml
        src/ui/qml/ui/OmDropdown.qml
        src/ui/qml/ui/OmTextField.qml
        src/ui/qml/ui/OmPopupCard.qml
        src/ui/qml/ui/OmListRow.qml
        src/ui/qml/ui/OmConfirm.qml
)
target_link_libraries(omarecord_ui PUBLIC Qt6::Quick Qt6::QuickControls2)
set_source_files_properties(
    src/ui/qml/Composition.qml
    src/ui/qml/Background.qml
    src/ui/qml/RoundedFrame.qml
    src/ui/qml/CursorOverlay.qml
    src/ui/qml/CameraOverlay.qml
    src/ui/qml/KeystrokeOverlay.qml
    src/ui/qml/Main.qml
    src/ui/qml/TopBar.qml
    src/ui/qml/PreviewPane.qml
    src/ui/qml/BottomBar.qml
    src/ui/qml/SidePanel.qml
    src/ui/qml/PanelSlider.qml
    src/ui/qml/PanelLabel.qml
    src/ui/qml/PanelHeading.qml
    src/ui/qml/PanelValue.qml
    src/ui/qml/IconToolButton.qml
    src/ui/qml/EditorButton.qml
    src/ui/qml/EditorSlider.qml
    src/ui/qml/EditorSwitch.qml
    src/ui/qml/EditorComboBox.qml
    src/ui/qml/BackgroundPanel.qml
    src/ui/qml/ShapePanel.qml
    src/ui/qml/CursorPanel.qml
    src/ui/qml/ZoomPanel.qml
    src/ui/qml/AudioPanel.qml
    src/ui/qml/ClipPanel.qml
    src/ui/qml/CameraPanel.qml
    src/ui/qml/KeystrokesPanel.qml
    src/ui/qml/ExportDialog.qml
    src/ui/qml/Timeline.qml
    src/ui/qml/ClipTrack.qml
    src/ui/qml/ZoomTrack.qml
    src/ui/qml/Launcher.qml
    src/ui/qml/RecordingBar.qml
    PROPERTIES QT_RESOURCE_ALIAS ""
)
set_source_files_properties(src/ui/qml/Composition.qml PROPERTIES QT_RESOURCE_ALIAS "Composition.qml")
set_source_files_properties(src/ui/qml/Background.qml PROPERTIES QT_RESOURCE_ALIAS "Background.qml")
set_source_files_properties(src/ui/qml/RoundedFrame.qml PROPERTIES QT_RESOURCE_ALIAS "RoundedFrame.qml")
set_source_files_properties(src/ui/qml/CursorOverlay.qml PROPERTIES QT_RESOURCE_ALIAS "CursorOverlay.qml")
set_source_files_properties(src/ui/qml/CameraOverlay.qml PROPERTIES QT_RESOURCE_ALIAS "CameraOverlay.qml")
foreach(qml_name IN ITEMS Main TopBar PreviewPane BottomBar SidePanel PanelSlider PanelLabel PanelHeading PanelValue IconToolButton EditorButton EditorSlider EditorSwitch EditorComboBox BackgroundPanel ShapePanel CursorPanel ZoomPanel AudioPanel ClipPanel CameraPanel KeystrokesPanel ExportDialog Timeline ClipTrack ZoomTrack Launcher RecordingBar)
    set_source_files_properties(src/ui/qml/${qml_name}.qml PROPERTIES QT_RESOURCE_ALIAS "${qml_name}.qml")
endforeach()
qt_add_qml_module(omarecord_render
    URI Omarecord
    VERSION 1.0
    RESOURCE_PREFIX /qt/qml
    OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/Omarecord
    SOURCES
        src/render/FrameSource.h src/render/FrameSource.cpp
        src/render/FfmpegDecoder.h src/render/FfmpegDecoder.cpp
        src/render/Exporter.h src/render/Exporter.cpp
        src/render/PreviewSink.h src/render/PreviewSink.cpp
        src/ui/Editor.h src/ui/Editor.cpp
        src/ui/Launcher.h src/ui/Launcher.cpp
        src/ui/RecordingBar.h src/ui/RecordingBar.cpp
    QML_FILES
        src/ui/qml/Composition.qml
        src/ui/qml/Background.qml
        src/ui/qml/RoundedFrame.qml
        src/ui/qml/CursorOverlay.qml
        src/ui/qml/CameraOverlay.qml
        src/ui/qml/KeystrokeOverlay.qml
        src/ui/qml/Main.qml
        src/ui/qml/TopBar.qml
        src/ui/qml/PreviewPane.qml
        src/ui/qml/BottomBar.qml
        src/ui/qml/SidePanel.qml
        src/ui/qml/PanelSlider.qml
        src/ui/qml/PanelLabel.qml
        src/ui/qml/PanelHeading.qml
        src/ui/qml/PanelValue.qml
        src/ui/qml/IconToolButton.qml
        src/ui/qml/EditorButton.qml
        src/ui/qml/EditorSlider.qml
        src/ui/qml/EditorSwitch.qml
        src/ui/qml/EditorComboBox.qml
        src/ui/qml/BackgroundPanel.qml
        src/ui/qml/ShapePanel.qml
        src/ui/qml/CursorPanel.qml
        src/ui/qml/ZoomPanel.qml
        src/ui/qml/AudioPanel.qml
        src/ui/qml/ClipPanel.qml
        src/ui/qml/CameraPanel.qml
        src/ui/qml/KeystrokesPanel.qml
        src/ui/qml/ExportDialog.qml
        src/ui/qml/Timeline.qml
        src/ui/qml/ClipTrack.qml
        src/ui/qml/ZoomTrack.qml
        src/ui/qml/Launcher.qml
        src/ui/qml/RecordingBar.qml
)
qt_add_resources(omarecord_render "omarecord_assets"
    PREFIX "/omarecord"
    FILES
        assets/cursors/arrow.svg assets/cursors/arrow-dark.svg assets/cursors/pointer.svg
        assets/gradients.json
        assets/icons/lucide/undo-2.svg assets/icons/lucide/redo-2.svg
        assets/icons/lucide/palette.svg assets/icons/lucide/square.svg
        assets/icons/lucide/mouse-pointer-2.svg assets/icons/lucide/zoom-in.svg
        assets/icons/lucide/zoom-out.svg
        assets/icons/lucide/film.svg assets/icons/lucide/camera.svg assets/icons/lucide/volume-2.svg
        assets/icons/lucide/play.svg assets/icons/lucide/pause.svg
        assets/icons/lucide/skip-back.svg assets/icons/lucide/skip-forward.svg
        assets/icons/lucide/scissors.svg assets/icons/lucide/crop.svg assets/icons/lucide/image.svg
        assets/icons/lucide/monitor.svg assets/icons/lucide/app-window.svg assets/icons/lucide/scan.svg
        assets/icons/lucide/save.svg assets/icons/lucide/download.svg assets/icons/lucide/x.svg
        assets/icons/lucide/check.svg assets/icons/lucide/plus.svg assets/icons/lucide/chevron-down.svg
        assets/icons/lucide/folder-open.svg assets/icons/lucide/settings-2.svg
        assets/icons/lucide/video.svg assets/icons/lucide/mic.svg assets/icons/lucide/mic-off.svg
        assets/icons/lucide/rotate-cw.svg assets/icons/lucide/flip-horizontal-2.svg
        assets/icons/lucide/circle.svg assets/icons/lucide/square-dashed.svg
        assets/icons/lucide/layers.svg
        assets/icons/lucide/keyboard.svg
        assets/sounds/click.wav
)
target_include_directories(omarecord_render PUBLIC src src/render)
target_link_libraries(omarecord_render PUBLIC omarecord_core omarecord_ui Qt6::Concurrent Qt6::Quick Qt6::QuickControls2 Qt6::Multimedia Qt6::OpenGL)

qt_add_executable(omarecord src/main.cpp)
target_link_libraries(omarecord PRIVATE omarecord_render Qt6::Quick Qt6::Multimedia LayerShellQt::Interface)
target_compile_definitions(omarecord PRIVATE OMARECORD_VERSION="${PROJECT_VERSION}")

include(CTest)
if(BUILD_TESTING)
    foreach(test_name IN ITEMS zoomtimeline cursorpath cliptimeline cameratimeline keystroketrack project inputlog regionpicker omarchypaths recorder)
        qt_add_executable(test_${test_name} tests/test_${test_name}.cpp)
        target_link_libraries(test_${test_name} PRIVATE omarecord_core Qt6::Test)
        add_test(NAME ${test_name} COMMAND test_${test_name})
    endforeach()
    qt_add_executable(test_exporter_smoke tests/test_exporter_smoke.cpp)
    target_link_libraries(test_exporter_smoke PRIVATE omarecord_render Qt6::Test)
    add_test(NAME exporter_smoke COMMAND test_exporter_smoke)
    qt_add_executable(test_editor tests/test_editor.cpp)
    target_link_libraries(test_editor PRIVATE omarecord_render Qt6::Test)
    add_test(NAME editor COMMAND test_editor)
endif()

install(TARGETS omarecord RUNTIME DESTINATION bin)
install(FILES pkg/omarecord.desktop DESTINATION share/applications)
