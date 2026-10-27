#!/usr/bin/env bash
# Builds (if needed) and runs the Qt comparison harness. Never touches the installed
# GUI: the program is built from this checkout into client/bench/build/qt and runs
# with temporary settings.
#
#   client/bench/run-qt-bench.sh [zerus-qt-bench options...]
# Options: --out FILE --label NAME --warmup-ms 2000 --run-ms 10000 --runs 3 --rows 2000
#          --events 1000 --step 16 --idle-ms 10000 --scenarios list-scroll,activity-scroll,...
#
# ZERUS_BENCH_DISPLAY selects where the window goes:
#   xvfb     (default) a private Xvfb; nothing reaches the desktop; not representative
#   wayland  the desktop's Wayland compositor (only with the owner warned first)
#   xcb      the desktop's X server or XWayland on $DISPLAY (only with the owner warned first)
# On a real display the window is titled "Zerus Qt measurement (closes itself)" and
# closes when the run ends. The run refuses to start while the session is locked or a
# display sleeps (a suspended window paints nothing), and adds the display modes to
# the --out file.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
build="$here/build/qt"
jobs="${CMAKE_BUILD_PARALLEL_LEVEL:-8}"
program="$build/zerus-qt-bench"

if [[ ! -x "$program" || "${ZERUS_BENCH_REBUILD:-0}" == 1 ]]; then
    cmake -S "$here/qt" -B "$build" -G Ninja -DCMAKE_BUILD_TYPE=Release >/dev/null
    nice -n 10 cmake --build "$build" --target zerus-qt-bench -j "$jobs"
fi

out=""
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
    [[ "${args[i]}" == --out ]] && out="${args[i + 1]:-}"
done

real_display() {
    python3 "$here/display-info.py" --require-visible
    "$@"
    if [[ -n "$out" ]]; then python3 "$here/display-info.py" --attach "$out"; fi
}

case "${ZERUS_BENCH_DISPLAY:-xvfb}" in
xvfb)
    # Drop every display and the session bus, then start a private Xvfb.
    exec env -u DISPLAY -u DBUS_SESSION_BUS_ADDRESS WAYLAND_DISPLAY=zerus-none \
        QT_QPA_PLATFORM=xcb ZERUS_BENCH_XVFB=1 \
        xvfb-run -a -s "-screen 0 1440x900x24" "$program" "$@"
    ;;
wayland)
    [[ -n "${WAYLAND_DISPLAY:-}" ]] || { echo "WAYLAND_DISPLAY is not set" >&2; exit 2; }
    real_display env -u DISPLAY -u ZERUS_BENCH_XVFB QT_QPA_PLATFORM=wayland "$program" "$@"
    ;;
xcb)
    [[ -n "${DISPLAY:-}" ]] || { echo "DISPLAY is not set" >&2; exit 2; }
    real_display env -u WAYLAND_DISPLAY -u ZERUS_BENCH_XVFB QT_QPA_PLATFORM=xcb "$program" "$@"
    ;;
*)
    echo "ZERUS_BENCH_DISPLAY must be xvfb, wayland or xcb" >&2
    exit 2
    ;;
esac
