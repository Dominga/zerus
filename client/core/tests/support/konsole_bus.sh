#!/bin/sh
# Runs the ignored `konsole_on_a_private_bus` test against a real Konsole that
# has its own home, configuration, runtime folder and session bus, rendered
# offscreen: nothing reaches the desktop's bus or screen. Afterwards it stops
# exactly the processes that carry this run's private marker. Usage, from
# client/core: tests/support/konsole_bus.sh TARGET_DIR
set -eu
target=${1:?target dir}
bin=$(cargo +beta test -j 8 --target-dir "$target" --features konsole --test terminal_launch \
    --no-run --message-format=json 2>/dev/null | python3 -c '
import json, sys
for line in sys.stdin:
    message = json.loads(line)
    if message.get("executable") and message["target"]["name"] == "terminal_launch":
        print(message["executable"])')
root=$(mktemp -d)
stop_marked() {
    for environ in /proc/[0-9]*/environ; do
        if { tr '\0' '\n' < "$environ"; } 2>/dev/null | grep -qx "ZERUS_TEST_KONSOLE_OUT=$1"; then
            pid=${environ#/proc/}; pid=${pid%/environ}
            kill "$pid" 2>/dev/null || true
        fi
    done
}
cleanup() {
    stop_marked "$root/off/out"
    stop_marked "$root/on/out"
    rm -rf "$root"
}
trap cleanup EXIT
run() {
    mode=$1
    home="$root/$mode"
    mkdir -p "$home/.config" "$home/run"
    chmod 700 "$home/run"
    if [ "$mode" = on ]; then
        printf '[KonsoleWindow]\nEnableSecuritySensitiveDBusAPI=true\n' > "$home/.config/konsolerc"
    fi
    env -u DISPLAY -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u TMUX \
        HOME="$home" XDG_CONFIG_HOME="$home/.config" XDG_RUNTIME_DIR="$home/run" \
        XDG_DATA_HOME="$home/.local/share" XDG_STATE_HOME="$home/.local/state" \
        XDG_CACHE_HOME="$home/.cache" QT_QPA_PLATFORM=offscreen SHELL=/bin/sh \
        ZERUS_TEST_KONSOLE="$mode" ZERUS_TEST_KONSOLE_OUT="$home/out" \
        dbus-run-session -- sh -c '
            konsole >/dev/null 2>&1 &
            for _ in $(seq 100); do
                dbus-send --session --print-reply --dest=org.freedesktop.DBus \
                    /org/freedesktop/DBus org.freedesktop.DBus.ListNames 2>/dev/null \
                    | grep -q org.kde.konsole- && break
                sleep 0.1
            done
            sleep 1
            "$0" konsole_on_a_private_bus --exact --ignored --nocapture
        ' "$bin"
    stop_marked "$home/out"
}
run off
run on
