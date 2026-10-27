#!/usr/bin/env bash
# Runs the client's pacing scenarios on the synthetic harness in WebKitGTK. Build the
# harness first:
#   (cd client && npx vite build --config bench/harness/vite.config.ts)
#   client/bench/run-webkitgtk.sh --out client/bench/results/FILE.json [--hz 60 --runs 3 ...]
#
# ZERUS_BENCH_DISPLAY selects where the window goes:
#   xvfb     (default) a private Xvfb, never the desktop. Xvfb has no vsync: WebKitGTK
#            paces frames with its own 60 Hz timer, so the numbers check the code path
#            and the engine's cost, not a display.
#   wayland  the desktop's compositor with GDK_BACKEND=wayland (only with the owner
#            warned first); pass --hz with the display's refresh rate. It refuses to
#            start while the session is locked or a display sleeps, and adds the
#            display modes to the --out file.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

out=""
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
    [[ "${args[i]}" == --out ]] && out="${args[i + 1]:-}"
done

case "${ZERUS_BENCH_DISPLAY:-xvfb}" in
xvfb)
    exec env -u WAYLAND_DISPLAY -u DISPLAY -u DBUS_SESSION_BUS_ADDRESS \
        GDK_BACKEND=x11 ZERUS_BENCH_XVFB=1 \
        xvfb-run -a -s "-screen 0 1440x900x24" \
        python3 "$here/run-webkitgtk.py" "$@"
    ;;
wayland)
    [[ -n "${WAYLAND_DISPLAY:-}" ]] || { echo "WAYLAND_DISPLAY is not set" >&2; exit 2; }
    python3 "$here/display-info.py" --require-visible
    env -u DISPLAY -u ZERUS_BENCH_XVFB GDK_BACKEND=wayland ZERUS_BENCH_DISPLAY=wayland \
        python3 "$here/run-webkitgtk.py" "$@"
    if [[ -n "$out" ]]; then python3 "$here/display-info.py" --attach "$out"; fi
    ;;
*)
    echo "ZERUS_BENCH_DISPLAY must be xvfb or wayland" >&2
    exit 2
    ;;
esac
