# Client measurement and the Qt comparison

Tooling for Beads `zerus-5wmh.4`: frame pacing, input-to-frame latency, session
open time, idle CPU and RSS of the new client against the tuned Qt client, and
the macOS pass/fail criteria of
`docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md` ("Risks and
measurement plan"). Everything here runs on synthetic data or reads `/proc`;
nothing replaces, restarts or reconfigures the installed GUI, `hgs`, native
agents or services.

| Path | What it is |
|---|---|
| `client/src/perf/pacing.ts` | rAF interval recorder, statistics and criterion 1 (`pacing.test.ts`) |
| `client/src/perf/scenarios.ts` | scripted scenarios, warm-up and worst-of-three runs, JSON report |
| `client/src/perf/Diagnostics.tsx` | in-app panel; also `globalThis.zerusPerf.run(options)` |
| `client/src-tauri/src/metrics.rs` | RSS and CPU% of the app and its WebKit processes |
| `client/src-tauri/src/platform/macos.rs` | WebKit display-rate preference, display-link counter |
| `client/perf-probe-mac/` | window-less macOS probe of the WebKit preference |
| `client/bench/qt/` | Qt harness: the tray's `SessionList` and `ActivityView` on synthetic data |
| `client/bench/run-qt-bench.sh` | builds and runs the Qt harness under a private Xvfb |
| `client/bench/sample-running-gui.py` | read-only CPU and memory sample of the running Qt GUI |
| `client/bench/harness/` | synthetic heavy screen that runs the client scenarios without Tauri |
| `client/bench/run-webkitgtk.sh` | runs that screen in WebKitGTK (Tauri's Linux engine) under Xvfb |
| `client/bench/metrics-test/` | compiles `metrics.rs` alone so its tests run without Tauri |
| `client/scripts/measure-report.py` | Russian results page from the result files |
| `client/bench/results/` | recorded numbers (no session names, no command lines) |

Result files share the schema `zerus-perf-v1`: `client` is `tauri` or `qt`,
`platform.label` names the machine and setup, `platform.representative` is false
for Xvfb or synthetic runs, and each scenario keeps all runs, `worst_index` and
`worst`.

## The client scenarios

Each scenario runs a 2 s warm-up and three 10 s runs; the worst run counts.

| Scenario | What is measured |
|---|---|
| `raf-light` | rAF intervals on the page as it is |
| `transform-animation` | rAF intervals while a CSS `transform` animation runs |
| `left-animation` | rAF intervals while a non-accelerated CSS `left` animation runs |
| `list-scroll` | `scrollTop` of the session list advanced 16 px each frame (bouncing at the ends); rAF and scroll-event intervals |
| `activity-scroll` | the same on the Activity |
| `session-open` | `openSession(i)` alternating between two rows, until the task after the next frame's paint |
| `input-latency` | `pointerdown` on a probe to the next rAF after the state change (and to after its paint); synthetic events at a random phase, or real clicks |

The worst pacing run is the one failing the most criteria, then with the larger
shares of slow frames; the worst timing run has the highest p95. The criteria
(`passes`) follow the spec: T = 1000 / display Hz, mean within T ± 1%, median
within T ± 0.5 ms, at least 99% of intervals at most 1.5 T, at most 0.2% above
2.5 T, under 1% in 15.0-18.5 ms. On a display that itself runs in that band
(60 Hz) the band check is not applicable.

T must come from the native side (`NSScreen.maximumFramesPerSecond` on macOS,
`platform::macos::screen_max_fps`), passed to `Diagnostics` as `displayHz`. The
rAF estimate is a fallback for other platforms only: on macOS it reads 60 exactly
when WebKit caps rendering.

In the app, mount `Diagnostics` with the app's scroll containers and session
opener, plus optional `displayHz`, `metrics` (the `metrics` command) and `save`.
A headless driver calls `globalThis.zerusPerf.run({ hz, hzSource: "manual" })`
and reads the report object.

### Without the app: the synthetic harness in WebKitGTK

```sh
(cd client && npx vite build --config bench/harness/vite.config.ts)   # needs client's vite and solid
client/bench/run-webkitgtk.sh --out client/bench/results/arch-webkitgtk-harness-DATE.json
```

The harness is a virtualized 2 000-row list (94 px cards, 40 px project headers)
and a 1 000-event Activity, served from a local cross-origin isolated server
(COOP and COEP, so `performance.now()` has 20 µs steps). After the run the driver
samples the WebKit processes for 10 s. Xvfb has no vsync, so WebKitGTK paces
frames with its own 60 Hz timer: these numbers check the code path and the
engine's cost, not a display.

## macOS: display rate

1. Window-less check of the SPI (safe over SSH, opens no window):

   ```sh
   rsync client/perf-probe-mac, client/src-tauri/src/{metrics.rs,platform/macos.rs} to a scratch directory
   cargo +1.99.1 run --release --manifest-path client/perf-probe-mac/Cargo.toml
   ```

   It prints the macOS version, the number of WebKit features, related keys, the
   key's default, the value in a new configuration before and after the change,
   and an untouched configuration. Recorded: `results/mac-webkit-probe-2026-10-28.txt`.

   `--display` also reads `NSScreen.maximumFramesPerSecond` and counts
   CVDisplayLink callbacks three times for 2 s; `--json FILE` writes the figures.
   A process started over SSH has no window server, so the display check runs as
   an app bundle that `open` starts in the owner's console session (no window, no
   Dock icon; only with the owner warned):

   ```sh
   client/bench/mac-bundle.sh target/release/zerus-client-perf-probe-mac bundles/ZerusDisplayProbe.app dev.zerus.next.displayprobe agent
   open -n -W --stdout "$PWD/probe.out" --stderr "$PWD/probe.err" bundles/ZerusDisplayProbe.app --args --display --json "$PWD/display.json"
   ```

   Recorded: `results/mac-display-2026-10-28.json`.
2. The pass/fail run (only after the coordinator has warned the owner): a release
   build of the client on the owner's ProMotion Mac, on AC power with Low Power
   Mode off and a nominal thermal state, the window fully visible. Run the
   Diagnostics scenarios with `webview_configuration` (preference disabled), then
   the control with `configure(mtm, true)` (preference kept), which must fail
   criterion 1, and record `display_link_hz` for the window's display
   (`window_display_id`); at least 117.6 Hz or the harness is invalid. Criterion 2
   (a window capture of at least 117.6 changed frames per second) needs a screen
   recording and is outside this panel.

## The Qt comparison

### The existing method

The Activity scroll numbers in `zerus-tllp` (224.7 to 14.8 ms per frame) and
`zerus-pgo9` (49.5 to 7.9 ms per frame) come from the Qt suites in
`tray/tests/test_activityview.cpp` (`richReplyScrollingKeepsQueuedInputResponsive`,
`tableReplyScrollingKeepsQueuedInputResponsive`): a synthetic reply in an
`ActivityView`, then per frame one `QWheelEvent` to the viewport,
`viewport()->repaint()` and `QCoreApplication::processEvents()`, timed with
`CLOCK_THREAD_CPUTIME_ID` (GUI-thread CPU), best of three runs of 20 frames. The
suites only assert an upper bound; the figures were read from those runs on the
Mac (Cocoa) and Linux.

### This harness

`client/bench/qt/qt_bench.cpp` keeps that method and widens it to the client's
scenarios. It links the tray's own static libraries (`hgs-tray-ui`,
`hgs-tray-activity`, `hgs-markdown`, `hgs-tray-core`) built from this checkout, so
it paints exactly what the installed GUI paints, and builds one window of
1440 x 900: a `SessionList` with `SessionDelegate` (compact 94 px cards, 40 px
project headers, 310 px wide, 2 000 rows in 100 projects) beside an
`ActivityView` with 1 000 events (prompts, tool results and rich replies with
headings, lists, inline code pills and a fenced block). Settings go to a
temporary directory.

| Scenario | Qt measurement | Client counterpart |
|---|---|---|
| `list-scroll` | per frame: scroll bar +16 px (bouncing), `viewport()->repaint()`, `processEvents()`; wall time and GUI-thread CPU | rAF interval while `scrollTop` advances 16 px per frame |
| `activity-scroll` | the same on the Activity browser | the same on the Activity |
| `session-open` | `setSessionKey` and `setActivity` of the other session until the render finished and the viewport repainted | `openSession` until after the next paint |
| `input-latency` | a posted mouse press on a probe label to the end of the label's paint showing the change | `pointerdown` to the next rAF |
| `idle` | process CPU over 10 s in a sleeping event loop, and RSS | the `metrics` command after an idle pause |

A 2 s warm-up and three 10 s runs per scenario, the run with the highest p95
counts. Qt widgets paint as soon as the event loop gets to them, without waiting
for the display, so the Qt frame figure is the work per frame (it shows the
headroom under a 8.33 ms or 16.67 ms budget), and the Qt input latency has no
frame wait in it; the page states this next to the numbers.

Build and run (only the harness target and the libraries it links; no Qt suites,
no native UI plugin; about 100 compile steps):

```sh
cmake -S client/bench/qt -B client/bench/build/qt -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build client/bench/build/qt --target zerus-qt-bench -j 8
client/bench/run-qt-bench.sh --label arch-xvfb --out /tmp/qt-bench.json
```

By default `run-qt-bench.sh` drops `DISPLAY`, `WAYLAND_DISPLAY` and the session
bus and runs the program on its own Xvfb with `QT_QPA_PLATFORM=xcb`. Options:
`--warmup-ms`, `--run-ms`, `--runs`, `--rows`, `--events`, `--step`, `--idle-ms`,
`--scenarios list-scroll,activity-scroll,session-open,input-latency,idle`.

Every scenario records `window_exposed`: a window that the compositor hid or
suspended paints nothing, and its numbers are invalid.

### On a real display

Only with the owner warned first. The windows are titled "... measurement
(closes itself)" and close when the run ends.

```sh
ZERUS_BENCH_DISPLAY=wayland client/bench/run-qt-bench.sh --label arch-kde-wayland --out /tmp/qt-wayland.json
ZERUS_BENCH_DISPLAY=xcb     client/bench/run-qt-bench.sh --label arch-kde-xwayland --out /tmp/qt-xcb.json
ZERUS_BENCH_DISPLAY=wayland client/bench/run-webkitgtk.sh --hz 144 --label arch-kde-webkitgtk --out /tmp/webkitgtk.json
```

Both scripts first run `display-info.py --require-visible`, which refuses while
the graphical session is locked or a display is in DPMS standby (KWin then
suspends new windows), and afterwards add the display modes from
`kscreen-doctor --json` (output name, mode, refresh rate, scale, priority; never
EDID, model or serial) to the result as `platform.displays`. Pass `--hz` with the
refresh rate of the display the window opens on.

On the Mac the same CMake project builds against Homebrew Qt as an app bundle
(`dev.zerus.next.qtbench`, apart from the installed app), and `open` starts it in
the console session:

```sh
cmake -S client/bench/qt -B build/qt -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH="$(brew --prefix qt)"
cmake --build build/qt --target zerus-qt-bench -j 6
open -n -W --stdout "$PWD/qt.out" --stderr "$PWD/qt.err" build/qt/zerus-qt-bench.app --args --label mac-promotion --out "$PWD/qt.json"
```

`client/bench/qt/packaging/Info.plist` is a placeholder: the tray's CMake
configures the (never built) `hgs-tray` bundle from `${CMAKE_SOURCE_DIR}/packaging`.

### The running GUI

Idle CPU and memory of the installed GUI come from a read-only sample of `/proc`
on Linux and of `ps` on macOS:

```sh
python3 client/bench/sample-running-gui.py --seconds 60 --label arch-live --out /tmp/qt-live.json
```

It finds the current user's single `hgs-tray`, reads the CPU time of it and its
descendants at the start and end of the window, and RSS (and PSS on Linux) at the
end. It records process names only, never command lines or paths, and never
signals or attaches to the process. "Idle" means no input from the harness; the
GUI keeps following its live sessions meanwhile, and the owner may be using it.

## Recorded results

| File | What |
|---|---|
| `results/arch-qt-2026-10-28.json` | Qt harness on Xvfb (not representative) and the running GUI on arch, 60 s |
| `results/arch-webkitgtk-harness-2026-10-28.json` | client scenarios on the synthetic harness, WebKitGTK 2.54.1 on Xvfb (not representative) |
| `results/arch-kde-display-2026-10-28.json` | arch display modes (two 144 Hz outputs at scale 1); real-display runs refused while the session was locked |
| `results/mac-qt-2026-10-28.json` | Qt harness on the ProMotion Mac (Cocoa, 120 Hz) and the running GUI, 60 s |
| `results/mac-display-2026-10-28.json` | WebKit preference, `NSScreen.maximumFramesPerSecond` and display-link rate on the Mac |
| `results/mac-webkit-probe-2026-10-28.txt` | text output of the window-less probe |

## The results page

```sh
python3 client/scripts/measure-report.py client/bench/results/*.json \
    --probe client/bench/results/mac-webkit-probe-2026-10-28.txt --out /tmp/zerus-measure.html
python3 -m unittest discover -s client/scripts -p "test_*.py"
```

The page is Russian, self-contained, uses no dot separators (the script refuses
to write one) and shows: per platform the criterion 1 table with pass or fail,
then Qt against the client (p95 frame time while scrolling the list and the
Activity, input-to-frame latency, session open time, idle CPU, RSS), then every
run. A Qt file that holds both the harness and a running-GUI sample shows as two
columns.
