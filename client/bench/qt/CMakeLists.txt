# Qt comparison harness for the client measurement (zerus-5wmh.4). Builds the tray's
# own static libraries from this checkout and one benchmark program; never the
# installed GUI. See client/bench/README.md.
#
#   cmake -S client/bench/qt -B client/bench/build/qt -G Ninja -DCMAKE_BUILD_TYPE=Release
#   cmake --build client/bench/build/qt --target zerus-qt-bench -j 8
cmake_minimum_required(VERSION 3.16)
project(zerus-qt-bench LANGUAGES CXX)

# Only the libraries the benchmark links: no Qt suites, no native UI plugin.
set(BUILD_TESTING OFF CACHE BOOL "Build the tray's Qt suites" FORCE)
set(HGS_NATIVE_UI_PLUGIN OFF CACHE STRING "Build the embedded native UI plugin" FORCE)
get_filename_component(ZERUS_TRAY_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../../tray" REALPATH)
add_subdirectory("${ZERUS_TRAY_DIR}" tray EXCLUDE_FROM_ALL)

find_package(Qt6 REQUIRED COMPONENTS Widgets)
add_executable(zerus-qt-bench qt_bench.cpp)
target_include_directories(zerus-qt-bench PRIVATE "${ZERUS_TRAY_DIR}/src")
target_link_libraries(zerus-qt-bench PRIVATE hgs-tray-ui hgs-tray-activity hgs-markdown hgs-tray-core Qt6::Widgets)
if(APPLE)
    # A bundle, so `open` starts it in the console session; its own identifier and
    # temporary settings keep it apart from the installed app.
    set_target_properties(zerus-qt-bench PROPERTIES MACOSX_BUNDLE TRUE
        MACOSX_BUNDLE_GUI_IDENTIFIER dev.zerus.next.qtbench MACOSX_BUNDLE_BUNDLE_NAME "Zerus Qt measurement")
endif()
