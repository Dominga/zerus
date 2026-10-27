#!/usr/bin/env bash
# WebKitGTK display-rate experiments on the real KDE Wayland desktop (Beads zerus-5wmh.14).
#
# Opens the synthetic harness window ("Zerus WebKitGTK measurement (closes itself)") once
# per configuration, a few seconds each, so run it only after the owner has been warned.
# Before every configuration it runs display-info.py --require-visible and stops when the
# session is locked or a display sleeps (KWin then suspends the window and nothing paints).
# Prints one verdict line per configuration with the mean rAF interval and what WebKit
# logged about its vblank monitor. See "WebKitGTK display rate on Linux" in README.md.
#
#   (cd client && npm ci && npx vite build --config bench/harness/vite.config.ts)
#   client/bench/webkitgtk-vblank-experiments.sh [--hz 144] [--only default,mm-shim] [--out-dir DIR]
#
# Configurations:
#   default             as Tauri runs it
#   debug-log           WEBKIT_DEBUG=DisplayLink, WEBKIT_DEBUG_OUTPUT=stderr (release logs only)
#   near60-off          debug-log plus the WebKit feature PreferPageRenderingUpdatesNear60FPS off
#   mm-shim             debug-log plus webkitgtk-monitor-mm-shim.so: GDK reports the kernel
#                       connector's physical size, so WebKit's DRM connector lookup can match
#   mm-shim-near60-off  both
#   xwayland            debug-log through XWayland (GDK_BACKEND=x11)
#   force-timer         debug-log plus WEBKIT_FORCE_VBLANK_TIMER=1 (control: the 60 Hz timer)
#
# --xvfb runs the same configurations on a private Xvfb instead (no desktop window, no
# visibility check, no xwayland configuration) to test this script; its numbers say
# nothing about the display.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

hz=144
scenarios="raf-light,transform-animation"
warmup_ms=1000
run_ms=3000
only=""
out_dir=""
xvfb=0
while [[ $# -gt 0 ]]; do
    case "$1" in
    --xvfb) xvfb=1; shift ;;
    --hz) hz="$2"; shift 2 ;;
    --scenarios) scenarios="$2"; shift 2 ;;
    --warmup-ms) warmup_ms="$2"; shift 2 ;;
    --run-ms) run_ms="$2"; shift 2 ;;
    --only) only="$2"; shift 2 ;;
    --out-dir) out_dir="$2"; shift 2 ;;
    -h | --help) sed -n '2,27p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done

if [[ "$xvfb" != 1 && ( -z "${WAYLAND_DISPLAY:-}" || "${WAYLAND_DISPLAY}" == zerus-none ) ]]; then
    echo "WAYLAND_DISPLAY is not set: run this in the desktop session" >&2; exit 2
fi
[[ -f "$here/build/harness/index.html" ]] || {
    echo "build the harness first: (cd client && npx vite build --config bench/harness/vite.config.ts)" >&2; exit 2; }
if [[ -z "$out_dir" ]]; then out_dir="$(mktemp -d "${TMPDIR:-/tmp}/zerus-webkitgtk-vblank.XXXXXX")"; fi
mkdir -p "$out_dir"

visible() {
    if [[ "$xvfb" == 1 ]]; then return 0; fi
    if ! python3 "$here/display-info.py" --require-visible; then
        echo "stopped before $1: the session is locked or a display sleeps" >&2
        echo "results so far: $out_dir" >&2
        exit 3
    fi
}

echo "== Display and DRM facts (no window)"
visible "the DRM probe"
probe_json="$out_dir/drm-probe.json"
python3 "$here/webkitgtk-drm-probe.py" --compositor | sed 's/^/  /'
python3 "$here/webkitgtk-drm-probe.py" --compositor --vblanks 0 --json > "$probe_json"
for param in /sys/module/nvidia_drm/parameters/vblank /sys/module/nvidia_drm/parameters/modeset; do
    if [[ -r "$param" ]]; then echo "  nvidia_drm.$(basename "$param")=$(cat "$param")"; fi
done

# GDK size -> kernel connector size for every output whose sizes differ (the shim's map).
mm_map="$(python3 -I - "$probe_json" <<'PY'
import json, sys
report = json.load(open(sys.argv[1]))
drm = {c["name"]: c["size_mm"] for n in report["nodes"] for c in n["connectors"]}
pairs = []
for o in report.get("compositor", {}).get("outputs", []):
    kernel = drm.get(o["name"])
    if kernel and kernel != o["size_mm"] and all(o["size_mm"]):
        pairs.append(f"{o['size_mm'][0]}x{o['size_mm'][1]}={kernel[0]}x{kernel[1]}")
print(",".join(pairs))
PY
)"
shim=""
if [[ -n "$mm_map" ]] && command -v cc >/dev/null; then
    mkdir -p "$here/build"
    cc -O2 -shared -fPIC -o "$here/build/webkitgtk-monitor-mm-shim.so" "$here/webkitgtk-monitor-mm-shim.c" -ldl
    shim="$here/build/webkitgtk-monitor-mm-shim.so"
    echo "  size shim map (GDK=DRM): $mm_map"
else
    echo "  size shim: not used (no size difference or no C compiler)"
fi

debug=(WEBKIT_DEBUG=DisplayLink WEBKIT_DEBUG_OUTPUT=stderr)
near60=(--disable-feature PreferPageRenderingUpdatesNear60FPS)

run_config() {
    local name="$1" mode="$2"; shift 2
    local extra_env=() extra_args=()
    while [[ $# -gt 0 && "$1" != -- ]]; do extra_env+=("$1"); shift; done
    if [[ "${1:-}" == -- ]]; then shift; fi
    extra_args=("$@")
    if [[ -n "$only" && ",$only," != *",$name,"* ]]; then return 0; fi
    visible "$name"
    local base_env wrap=()
    if [[ "$xvfb" == 1 ]]; then
        [[ "$mode" == wayland ]] || { echo "$name: skipped with --xvfb"; return 0; }
        # Never the desktop: no DISPLAY, a Wayland socket name that does not exist, GTK forced to X11.
        base_env=(-u DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u ZERUS_BENCH_DISPLAY WAYLAND_DISPLAY=zerus-none
            GDK_BACKEND=x11 ZERUS_BENCH_XVFB=1)
        wrap=(xvfb-run -a -s "-screen 0 1440x900x24")
    elif [[ "$mode" == wayland ]]; then
        base_env=(-u DISPLAY -u ZERUS_BENCH_XVFB GDK_BACKEND=wayland ZERUS_BENCH_DISPLAY=wayland)
    else
        [[ -n "${DISPLAY:-}" ]] || { echo "$name: skipped, DISPLAY (XWayland) is not set"; return 0; }
        base_env=(-u WAYLAND_DISPLAY -u ZERUS_BENCH_XVFB GDK_BACKEND=x11 ZERUS_BENCH_DISPLAY=xwayland)
    fi
    local out="$out_dir/$name.json" log="$out_dir/$name.log" status=0
    rm -f "$out"
    timeout 180 env "${base_env[@]}" "${wrap[@]}" env "${extra_env[@]}" python3 "$here/run-webkitgtk.py" \
        --out "$out" --label "vblank-$name" --hz "$hz" --scenarios "$scenarios" \
        --warmup-ms "$warmup_ms" --run-ms "$run_ms" --runs 1 --idle-seconds 1 --timeout 120 \
        "${extra_args[@]}" 2> "$log" || status=$?
    python3 -I - "$name" "$out" "$log" "$hz" "$status" <<'PY'
import json, os, re, sys
name, out, log, hz, status = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4]), int(sys.argv[5])
T = 1000.0 / hz

def verdict(median):
    # By the median: a few short intervals at the start of a 3 s run pull the mean down.
    if median is None:
        return "no frames"
    if abs(median - T) <= 0.02 * T:
        return f"display rate ({hz:g} Hz)"
    if abs(median - 2 * T) <= 0.02 * 2 * T:
        return f"half the display rate ({hz / 2:g} Hz, the near-60 preference)"
    if 15.9 <= median <= 16.3:
        return "60 Hz timer (WebKit sleeps 16 ms)"
    if 16.45 <= median <= 16.9:
        return "60 Hz vblank"
    return "other"

lines = []
try:
    text = open(log, encoding="utf-8", errors="replace").read()
except OSError:
    text = ""
for line in text.splitlines():
    if "Unknown logging channel" in line:
        continue
    if re.search(r"vblank|VBlank|DisplayLink|timer|zerus-mm-shim|THROTTLE|refusing", line):
        line = re.sub(r" \[/\S*/(Source/\S+) \w+\]$", r" [\1]", line)
        if line not in lines:
            lines.append(line)
if status or not os.path.exists(out):
    tail = [l for l in text.splitlines() if l.strip() and "Unknown logging channel" not in l][-3:]
    print(f"{name:19} FAILED (exit {status}): {' | '.join(tail)}")
    sys.exit(0)
report = json.load(open(out))
parts, verdicts = [], []
for s in report["scenarios"]:
    worst = s.get("worst") or {}
    iv = worst.get("interval_ms") or {}
    mean, median = iv.get("mean"), iv.get("median")
    parts.append(f"{s['id']} mean {mean:.2f} ms, median {median:.2f} ms" if mean and median else f"{s['id']} no frames")
    verdicts.append(verdict(median if mean else None))
webkit = report.get("platform", {}).get("webkit", {})
mon = webkit.get("gdk_monitor") or {}
mon_text = f"GDK monitor {mon.get('size_mm')} mm, {mon.get('refresh_mhz')} mHz" if mon else "GDK monitor unknown"
summary = verdicts[0] if len(set(verdicts)) == 1 else ", ".join(verdicts)
print(f"{name:19} {'; '.join(parts)} -> {summary} ({mon_text})")
for line in lines[:6]:
    print(f"{'':19}   {line}")
PY
    if [[ "$xvfb" != 1 ]] && ! python3 "$here/display-info.py" --require-visible 2>/dev/null; then
        echo "$name: the session locked or a display slept during the run, so its numbers are invalid" >&2
        visible "the next configuration"
    fi
}

echo "== Configurations ($scenarios, ${run_ms} ms each, T = $(python3 -c "print(round(1000/$hz, 2))") ms)"
run_config default wayland
run_config debug-log wayland "${debug[@]}"
run_config near60-off wayland "${debug[@]}" -- "${near60[@]}"
if [[ -n "$shim" ]]; then
    run_config mm-shim wayland "${debug[@]}" "LD_PRELOAD=$shim" "ZERUS_GDK_MONITOR_MM=$mm_map"
    run_config mm-shim-near60-off wayland "${debug[@]}" "LD_PRELOAD=$shim" "ZERUS_GDK_MONITOR_MM=$mm_map" -- "${near60[@]}"
fi
run_config xwayland xwayland "${debug[@]}"
run_config force-timer wayland "${debug[@]}" WEBKIT_FORCE_VBLANK_TIMER=1
echo "results: $out_dir"
