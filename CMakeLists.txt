cmake_minimum_required(VERSION 3.22)
project(omareel VERSION 0.5.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Concurrent Gui GuiPrivate Quick QuickControls2 Multimedia OpenGL ShaderTools Svg Test)
find_package(LayerShellQt REQUIRED)

# The program logic is Rust (crates/). omareel_qt is the one static library linked into C++:
# it carries omareel_core, omareel_record, the CXX-Qt objects that expose them to QML, and the
# C++ in crates/omareel_qt/cpp that calls into them (the editor's base class and the exporter, and
# the launcher's and recording bar's base classes).
find_package(Corrosion REQUIRED)
include(cmake/CxxQt.cmake)
cxx_qt_import_crate(MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml CRATES omareel_qt LOCKED
    QT_MODULES Qt6::Core Qt6::Gui Qt6::Multimedia Qt6::Quick)
target_include_directories(omareel_qt INTERFACE ${CMAKE_CURRENT_SOURCE_DIR}/crates/omareel_qt/cpp)
find_package(PkgConfig REQUIRED)
option(OMAREEL_BUILD_HYPRLAND_PLUGIN "Build the matching Hyprland capture exclusion plugin" ON)
if(OMAREEL_BUILD_HYPRLAND_PLUGIN)
    pkg_check_modules(HYPRLAND QUIET hyprland)
    if(HYPRLAND_FOUND)
        add_subdirectory(plugin)
    endif()
endif()
pkg_check_modules(TURBOJPEG REQUIRED IMPORTED_TARGET libturbojpeg)
# libevdev is what omareel_record reads the input devices with, and libxkbcommon what it names
# the keys with through the user's layout; the screen capture's Wayland client is Rust's own.
# libwayland-client is for the windows main.cpp commits itself.
pkg_check_modules(LIBEVDEV REQUIRED IMPORTED_TARGET libevdev)
pkg_check_modules(XKBCOMMON REQUIRED IMPORTED_TARGET xkbcommon)
# The screen capture's GPU side (crates/omareel_record/csrc): GBM buffers, VA-API encoding through
# FFmpeg's libraries. Cargo builds it; the executable links what it calls, except FFmpeg's
# libraries, which omareel_core opens when it first needs them (crates/omareel_core/csrc/
# ffmpeg_dl.h): that takes about 90 libraries off every omareel process's start.
pkg_check_modules(GPU_CAPTURE REQUIRED IMPORTED_TARGET gbm libdrm libva)
pkg_check_modules(WAYLAND_CLIENT REQUIRED IMPORTED_TARGET wayland-client)
qt_standard_project_setup()
qt_policy(SET QTP0004 NEW)

# C++ the Rust code drives: Qt-only pieces such as the editor's players, the export renderer,
# the camera, file watchers and font calls. It never calls into Rust, so the static libraries link
# in one direction.
add_library(omareel_plumbing STATIC
    src/plumbing/QtShims.cpp
    src/plumbing/ThemeBase.cpp
    src/plumbing/PlaybackDriver.cpp
    src/plumbing/ExportRenderer.cpp
    src/plumbing/CameraCapture.cpp
)
target_include_directories(omareel_plumbing PUBLIC src/plumbing)
target_link_libraries(omareel_plumbing
    PUBLIC Qt6::Core Qt6::Gui Qt6::Multimedia Qt6::Qml Qt6::Quick Qt6::OpenGL
    PRIVATE PkgConfig::TURBOJPEG)
target_link_libraries(omareel_qt INTERFACE omareel_plumbing PkgConfig::LIBEVDEV PkgConfig::XKBCOMMON PkgConfig::GPU_CAPTURE)

add_library(omareel_render STATIC)
qt_add_qml_module(omareel_ui STATIC
    URI Omareel.Ui
    VERSION 1.0
    RESOURCE_PREFIX /qt/qml
    OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/Omareel/Ui
    QML_FILES
        src/ui/qml/ui/OmButton.qml
        src/ui/qml/ui/OmIconButton.qml
        src/ui/qml/ui/OmButtonGroup.qml
        src/ui/qml/ui/OmToggle.qml
        src/ui/qml/ui/OmSlider.qml
        src/ui/qml/ui/OmTrimHandle.qml
        src/ui/qml/ui/OmSectionHeader.qml
        src/ui/qml/ui/OmHero.qml
        src/ui/qml/ui/OmSeparator.qml
        src/ui/qml/ui/OmDropdown.qml
        src/ui/qml/ui/OmTextField.qml
        src/ui/qml/ui/OmPopupCard.qml
        src/ui/qml/ui/OmListRow.qml
        src/ui/qml/ui/OmConfirm.qml
        src/ui/qml/ui/OmToolTip.qml
        src/ui/qml/ui/OmPopup.qml
        src/ui/qml/ui/OmMenu.qml
        src/ui/qml/ui/OmMenuItem.qml
        src/ui/qml/ui/OmMenuSeparator.qml
        src/ui/qml/ui/OmKeycap.qml
        src/ui/qml/ui/OmSettingRow.qml
        src/ui/qml/ui/OmColorWell.qml
        src/ui/qml/ui/OmEmptyState.qml
        src/ui/qml/ui/OmCountModel.qml
        src/ui/qml/ui/OmLazyToolTip.qml
)
target_link_libraries(omareel_ui PUBLIC Qt6::Quick Qt6::QuickControls2)
set_source_files_properties(
    src/ui/qml/Composition.qml
    src/ui/qml/Background.qml
    src/ui/qml/RoundedFrame.qml
    src/ui/qml/CursorOverlay.qml
    src/ui/qml/CameraOverlay.qml
    src/ui/qml/BlurredBackdrop.qml
    src/ui/qml/MediaLayer.qml
    src/ui/qml/KeystrokeOverlay.qml
    src/ui/qml/CaptionOverlay.qml
    src/ui/qml/OverlayLayer.qml
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
    src/ui/qml/SpeechPanel.qml
    src/ui/qml/AgentDrawer.qml
    src/ui/qml/ExportDialog.qml
    src/ui/qml/InsertClipDialog.qml
    src/ui/qml/Timeline.qml
    src/ui/qml/ClipTrack.qml
    src/ui/qml/ZoomTrack.qml
    src/ui/qml/OverlayTrack.qml
    src/ui/qml/OverlayEditLayer.qml
    src/ui/qml/OverlayPanel.qml
    src/ui/qml/Launcher.qml
    src/ui/qml/RecordingBar.qml
    PROPERTIES QT_RESOURCE_ALIAS ""
)
set_source_files_properties(src/ui/qml/Composition.qml PROPERTIES QT_RESOURCE_ALIAS "Composition.qml")
set_source_files_properties(src/ui/qml/Background.qml PROPERTIES QT_RESOURCE_ALIAS "Background.qml")
set_source_files_properties(src/ui/qml/RoundedFrame.qml PROPERTIES QT_RESOURCE_ALIAS "RoundedFrame.qml")
set_source_files_properties(src/ui/qml/CursorOverlay.qml PROPERTIES QT_RESOURCE_ALIAS "CursorOverlay.qml")
set_source_files_properties(src/ui/qml/CameraOverlay.qml PROPERTIES QT_RESOURCE_ALIAS "CameraOverlay.qml")
set_source_files_properties(src/ui/qml/BlurredBackdrop.qml PROPERTIES QT_RESOURCE_ALIAS "BlurredBackdrop.qml")
set_source_files_properties(src/ui/qml/MediaLayer.qml PROPERTIES QT_RESOURCE_ALIAS "MediaLayer.qml")
set_source_files_properties(src/ui/qml/OverlayLayer.qml PROPERTIES QT_RESOURCE_ALIAS "OverlayLayer.qml")
foreach(qml_name IN ITEMS Main TopBar PreviewPane BottomBar SidePanel PanelSlider PanelLabel PanelHeading PanelValue IconToolButton EditorButton EditorSlider EditorSwitch EditorComboBox BackgroundPanel ShapePanel CursorPanel ZoomPanel AudioPanel ClipPanel CameraPanel KeystrokesPanel SpeechPanel ExportDialog InsertClipDialog Timeline ClipTrack ZoomTrack OverlayTrack OverlayEditLayer OverlayPanel Launcher RecordingBar)
    set_source_files_properties(src/ui/qml/${qml_name}.qml PROPERTIES QT_RESOURCE_ALIAS "${qml_name}.qml")
endforeach()
qt_add_qml_module(omareel_render
    URI Omareel
    VERSION 1.0
    RESOURCE_PREFIX /qt/qml
    OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/Omareel
    SOURCES
        src/render/FrameSource.h src/render/FrameSource.cpp
        src/render/VideoDecoding.h src/render/VideoDecoding.cpp
    QML_FILES
        src/ui/qml/Composition.qml
        src/ui/qml/Background.qml
        src/ui/qml/RoundedFrame.qml
        src/ui/qml/CursorOverlay.qml
        src/ui/qml/CameraOverlay.qml
        src/ui/qml/BlurredBackdrop.qml
        src/ui/qml/MediaLayer.qml
        src/ui/qml/KeystrokeOverlay.qml
        src/ui/qml/CaptionOverlay.qml
        src/ui/qml/OverlayLayer.qml
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
        src/ui/qml/SpeechPanel.qml
        src/ui/qml/AgentDrawer.qml
        src/ui/qml/ExportDialog.qml
        src/ui/qml/InsertClipDialog.qml
        src/ui/qml/Timeline.qml
        src/ui/qml/ClipTrack.qml
        src/ui/qml/ZoomTrack.qml
        src/ui/qml/OverlayTrack.qml
        src/ui/qml/OverlayEditLayer.qml
        src/ui/qml/OverlayPanel.qml
        src/ui/qml/Launcher.qml
        src/ui/qml/RecordingBar.qml
)
# FrameSource's YUV node converts the export's frames on the GPU.
qt_add_shaders(omareel_render "omareel_shaders"
    BATCHABLE
    PREFIX "/omareel/shaders"
    BASE src/render/shaders
    FILES src/render/shaders/yuv.vert src/render/shaders/yuv.frag
)
# The preview's pixelated blur regions sample mip levels (textureLod), which ESSL 100 lacks.
qt_add_shaders(omareel_render "omareel_overlay_shaders"
    GLSL "300es,330"
    PREFIX "/omareel/shaders"
    BASE src/render/shaders
    FILES src/render/shaders/pixelate.frag
)
qt_add_resources(omareel_render "omareel_assets"
    PREFIX "/omareel"
    FILES
        assets/cursors/arrow.svg assets/cursors/arrow-dark.svg assets/cursors/pointer.svg
        assets/gradients.json
        assets/icons/lucide/undo-2.svg assets/icons/lucide/redo-2.svg
        assets/icons/lucide/palette.svg assets/icons/lucide/square.svg
        assets/icons/lucide/mouse-pointer-2.svg assets/icons/lucide/zoom-in.svg
        assets/icons/lucide/zoom-out.svg
        assets/icons/lucide/type.svg assets/icons/lucide/arrow-up-right.svg assets/icons/lucide/hash.svg
        assets/icons/lucide/pen-line.svg assets/icons/lucide/presentation.svg
        assets/icons/lucide/film.svg assets/icons/lucide/camera.svg assets/icons/lucide/volume-2.svg
        assets/icons/lucide/captions.svg
        assets/icons/lucide/play.svg assets/icons/lucide/pause.svg assets/icons/lucide/flag.svg
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
        assets/icons/lucide/eye.svg assets/icons/lucide/eye-off.svg assets/icons/lucide/trash-2.svg
        assets/icons/lucide/video-off.svg assets/icons/lucide/volume-x.svg
        assets/icons/lucide/sparkles.svg assets/icons/lucide/music.svg
        assets/icons/lucide/copy.svg assets/icons/lucide/message-square.svg assets/icons/lucide/bookmark.svg
        assets/icons/reel.svg
        assets/fonts/Inter-Regular.ttf assets/fonts/Inter-Medium.ttf
        assets/fonts/Inter-SemiBold.ttf assets/fonts/Inter-Bold.ttf
        assets/sounds/click.wav
)
target_include_directories(omareel_render PUBLIC src src/render)
target_link_libraries(omareel_render PUBLIC omareel_qt omareel_ui Qt6::Concurrent Qt6::Quick Qt6::QuickControls2 Qt6::Multimedia Qt6::OpenGL Qt6::Svg
    PRIVATE Qt6::GuiPrivate)

qt_add_executable(omareel src/main.cpp src/ui/CaptureTestClient.cpp)
target_link_libraries(omareel PRIVATE omareel_render omareel_qt Qt6::Quick Qt6::Multimedia Qt6::GuiPrivate LayerShellQt::Interface PkgConfig::WAYLAND_CLIENT)
target_compile_definitions(omareel PRIVATE OMAREEL_VERSION="${PROJECT_VERSION}")
add_custom_command(TARGET omareel POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E rm -f $<TARGET_FILE_DIR:omareel>/omarecord
    COMMAND ${CMAKE_COMMAND} -E create_symlink omareel $<TARGET_FILE_DIR:omareel>/omarecord)

include(CTest)
if(BUILD_TESTING)
    # The camera's frame packing, timestamp file, pacing and device matching: plumbing, which
    # needs no Rust.
    qt_add_executable(test_cameracapture tests/test_cameracapture.cpp)
    target_link_libraries(test_cameracapture PRIVATE omareel_plumbing Qt6::Test)
    add_test(NAME cameracapture COMMAND test_cameracapture)
    qt_add_executable(test_exporter_smoke tests/test_exporter_smoke.cpp)
    target_link_libraries(test_exporter_smoke PRIVATE omareel_render Qt6::Test)
    add_test(NAME exporter_smoke COMMAND test_exporter_smoke)
    find_package(Qt6 REQUIRED COMPONENTS Network)
    # The blur test again on the software scene graph (offscreen), whatever the run's platform.
    add_test(NAME exporter_overlays_software COMMAND test_exporter_smoke blurRegionsHideWhatTheyCover blurNeverLeaksAFrame)
    set_tests_properties(exporter_overlays_software PROPERTIES ENVIRONMENT QT_QPA_PLATFORM=offscreen)
    qt_add_executable(test_editor tests/test_editor.cpp)
    target_link_libraries(test_editor PRIVATE omareel_render Qt6::Test Qt6::Network)
    add_test(NAME editor COMMAND test_editor)
    # `omareel agent` with a fake agent in place of Claude Code and Codex (no tokens spent).
    find_program(OMAREEL_PYTHON3 python3)
    if(OMAREEL_PYTHON3)
        add_test(NAME agent_fake COMMAND sh ${CMAKE_CURRENT_SOURCE_DIR}/tests/agent_fake_e2e.sh
            $<TARGET_FILE:omareel> ${CMAKE_CURRENT_SOURCE_DIR})
    endif()
    # The Rust crates' own tests. omareel_qt is left out: it only links inside a Qt binary.
    set(OMAREEL_CARGO_TEST ${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/cargo/test
        ${Rust_CARGO} test --locked --workspace --exclude omareel_qt --quiet)
    add_custom_target(omareel_rust_tests ALL COMMAND ${OMAREEL_CARGO_TEST} --no-run
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR} USES_TERMINAL VERBATIM)
    add_test(NAME rust COMMAND ${OMAREEL_CARGO_TEST} WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR})
    # The capture exclusion plugin's own function hook, which only aarch64 uses.
    if(CMAKE_SYSTEM_PROCESSOR MATCHES "^(aarch64|arm64)$")
        add_executable(test_arm64hook plugin/test_arm64hook.cpp)
        add_test(NAME arm64hook COMMAND test_arm64hook)
    endif()
endif()

install(TARGETS omareel RUNTIME DESTINATION bin)
install(FILES pkg/omareel.desktop DESTINATION share/applications)
install(FILES pkg/omareel.svg DESTINATION share/icons/hicolor/scalable/apps)
foreach(icon_size IN ITEMS 16 32 48 64 128 256)
    install(FILES pkg/icons/omareel-${icon_size}.png
            DESTINATION share/icons/hicolor/${icon_size}x${icon_size}/apps
            RENAME omareel.png)
endforeach()
install(CODE "execute_process(COMMAND \"${CMAKE_COMMAND}\" -E create_symlink omareel \"\$ENV{DESTDIR}${CMAKE_INSTALL_PREFIX}/bin/omarecord\")")
