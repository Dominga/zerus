> Qt baseline commands and paths below are historical measurements. The Qt
> harness has been removed at retirement; retain the recorded JSON for comparison,
> and use the maintained Tauri harness for new runs. Do not rebuild an old Qt GUI.

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

## WebKitGTK display rate on Linux

Beads `zerus-5wmh.14`. On arch (KDE Plasma 6.7.5 on Wayland, NVIDIA RTX 3090 Ti
with the open nvidia-drm 615.78.08 and `modeset=Y`, two outputs at 144 Hz and
scale 1, webkit2gtk-4.1 2.54.1) requestAnimationFrame, CSS `transform` and `left`
animations and list scrolling run at 16.04 to 16.06 ms
(`results/arch-kde-webkitgtk-harness-2026-10-28.json`, medians), in the harness and
in the Tauri client. Qt on the same desktop follows the 144 Hz outputs. The
interval is WebKit's fallback timer, not a display: a 60 Hz vblank gives 16.67 ms.

### How WebKitGTK paces rendering

File and line references are to the `webkitgtk-2.54.1.tar.xz` release
(sha256 `ea0bbb02dbdbc596874a4e7ad35b66645b3e0a232bd0e4081de5ed92eb0a397d`). In `Source/WebKit/UIProcess/` unless the path says otherwise.

- The UI process (the Tauri app itself) runs one `DisplayLink` per display. The web
  process asks for its callbacks over IPC (`Source/WebKit/WebProcess/WebPage/WebDisplayRefreshMonitor.cpp:78-84`),
  and rAF, animations and scroll updates run when they fire.
- `glib/DisplayLinkGLib.cpp:35-65` builds the link from `DisplayVBlankMonitor::create(displayID)`
  and takes that monitor's refresh rate as the display's nominal rate.
- `glib/DisplayVBlankMonitor.cpp:48-70`. It returns the timer when the display ID is 0 or
  `WEBKIT_FORCE_VBLANK_TIMER` is set and not `0`. Otherwise it tries the DRM monitor.
  When that fails, it logs `Failed to create DRM vblank monitor, falling back to timer`
  (line 67) and returns the timer.
- `glib/DisplayVBlankMonitorTimer.cpp:40-49`. The timer's rate is `FullSpeedFramesPerSecond`
  = 60 (`Source/WebCore/platform/graphics/AnimationFrameRate.h:56`), and each tick is
  `sleep_for(milliseconds(1000 / 60))`, 16 ms. That is the 16.06 ms we measured.
- `glib/DisplayVBlankMonitorDRM.cpp`. `create` (209-243) takes the GdkMonitor of the
  display. `findDrmNodeWithCrtc` (170-198) then opens every DRM primary node with
  `O_RDWR`. For GTK, `findCrtc` (51-105) accepts only a connector that is connected
  and has an `encoder_id` and modes (line 67). Its `mmWidth` and `mmHeight` must also
  equal `gdk_monitor_get_width_mm` and `gdk_monitor_get_height_mm` (line 72). The CRTC
  index comes from the encoder's `crtc_id` (88-95). The refresh rate is
  `gdk_monitor_get_refresh_rate() / 1000` (92, 242). One `drmWaitVBlank` must then
  succeed (232-240). After that, the `VBlankMonitor` thread waits for one vblank per frame
  (`glib/DisplayVBlankMonitorThreaded.cpp:53-80`). WebKit's main branch (2026-10-28) has
  the same lookup.
- GTK 3 on Wayland sets the GdkMonitor's physical size from `wl_output.geometry`
  (`gdk/wayland/gdkscreen-wayland.c:1730-1735` in GTK 3.24.52) and its refresh rate from
  `wl_output.mode` (1797). XWayland should take its RandR output sizes from `wl_output`
  too, so X11 is expected to fail the same way (the runner's `xwayland` configuration checks it).
- No environment variable selects the DRM monitor or sets the timer's rate.
  `WEBKIT_DISPLAY_REFRESH_THROTTLE_FPS` only lowers the rate to a factor of the
  monitor's rate (`glib/DisplayLinkGLib.cpp:49-64`).
- With working vblanks there is still a cap. `PreferPageRenderingUpdatesNear60FPSEnabled`
  is on by default everywhere except visionOS
  (`Source/WTF/Scripts/Preferences/UnifiedWebPreferences.yaml:6816-6831`); it is the same
  preference the macOS client turns off. `Source/WebCore/page/Page.cpp:2773` and
  `Source/WebCore/platform/graphics/AnimationFrameRate.cpp:35-45,60` turn 144 into 72
  (integer ratio 144 / 60 = 2). `DisplayLink.cpp:207-214` with
  `Source/WebCore/platform/graphics/DisplayUpdate.cpp:35-43` then fires every second
  vblank, 13.89 ms. GTK can turn it off per web view with `webkit_settings_set_feature_enabled`
  (`API/glib/WebKitSettings.cpp:4234-4240`, since 2.42). The feature identifier is
  `PreferPageRenderingUpdatesNear60FPS`, without `Enabled`.
- Logging. `WEBKIT_DEBUG=DisplayLink WEBKIT_DEBUG_OUTPUT=stderr` prints the release logs
  above. Channels are off unless `WEBKIT_DEBUG` names them
  (`Source/WTF/wtf/unix/LoggingUnix.cpp:36-50`). Output goes to the journal when
  `/run/systemd/journal/socket` exists (`Source/WTF/wtf/Assertions.cpp:500-517`). The
  debug-only `LOG_WITH_STREAM` lines, such as the nominal rate, are not in release
  builds. The installed library has the DRM monitor: it links libdrm, and its strings
  hold the messages from `DisplayVBlankMonitorDRM.cpp:214, 226, 238`.
- Error text. libdrm returns -1 and sets `errno`, so WebKit's
  `drmWaitVBlank failed: %s` with `strerror(-ret)` always reads "Operation not
  permitted" (line 238). The thread likewise treats every failure as `-EPERM` (259-264).
  Read the real error from the probe below.

### This machine

WebKit's checks in order, with what `webkitgtk-drm-probe.py` and the system show:

| Check | Result here |
|---|---|
| a connector of the GdkMonitor's size (`findCrtc`, line 72) | fails. KWin reports 597x336 and 797x334 mm (`wl_output`, also `kscreen-doctor` `sizeMM`). The kernel connectors DP-2 and DP-3 report 600x340 and 800x330 mm, in whole centimetres. WebKit logs `no drm node with CRTC found` (226), and this alone gives the 16.06 ms timer |
| `drmWaitVBlank` (232-240) | fails. nvidia-drm's module parameter `vblank` is `N`, its default ("Enable drm vblank notification support"), and every CRTC answers `EOPNOTSUPP`. KWin paces with page-flip events and does not need it |
| connector routing for a non-master client (`encoder_id`, line 67) | unknown. While the session was locked and the outputs were in DPMS standby, no connector had a CRTC: `encoder_id` 0, atomic `CRTC_ID` 0. It needs a probe with the displays on |
| near-60 preference | would give 72 Hz once vblanks work |

`/dev/dri` has only `card1` (the NVIDIA GPU, PCI 01:00.0). The AMD Raphael iGPU
(11:00.0) has no DRM driver bound and drives no output, so it offers no path. Ranked:

1. Timer fallback because no connector has the GdkMonitor's size. The source and the
   device facts show it. Under Xvfb, WebKit logs the same fallback lines (226, 67);
   the runner's `debug-log` configuration checks them on the display.
2. A size fix alone is not enough: nvidia-drm without `vblank=1` fails `drmWaitVBlank`,
   so WebKit still uses the timer.
3. With both fixed, the near-60 preference halves 144 Hz to 72 Hz unless the web view
   turns it off.
4. Unlikely: the compositor or the GTK frame clock. Qt follows 144 Hz on the same
   desktop, and WebKit drives rAF from its own vblank thread, not from frame callbacks.

So WebKitGTK 2.54.1 cannot reach 144 Hz here by configuration alone. It needs all of:
`nvidia_drm.vblank=1` (a kernel module option, the owner's decision, active after a
reboot); a connector lookup that matches (a WebKit fix, or the shim for the
experiment); connector routing visible to the app; and
`PreferPageRenderingUpdatesNear60FPS` off in the client's web view. On drivers with
DRM vblank support (amdgpu, i915), only the size match and the preference remain, and
the size match depends on the monitor's EDID.

### The experiments runner

Only after the owner has been warned. It opens the harness window once per
configuration, about 8 s each, and stops when the session is locked or a display
sleeps:

```sh
(cd client && npm ci && npx vite build --config bench/harness/vite.config.ts)
client/bench/webkitgtk-vblank-experiments.sh --hz 144
```

First it prints the window-less DRM probe
(`python3 client/bench/webkitgtk-drm-probe.py --compositor`: connector sizes,
`encoder_id`, atomic `CRTC_ID`, `drmWaitVBlank` on every CRTC, the compositor's sizes,
the nvidia-drm parameters). It then prints one line per configuration: the mean and
median rAF interval, a verdict, the GdkMonitor's size and refresh rate, and WebKit's
vblank log lines. JSON results and logs go to a temporary directory. `--only NAME,...`
picks configurations, and `--xvfb` checks the script on a private Xvfb (no desktop).

| Configuration | Expected here (nvidia-drm `vblank=N`) |
|---|---|
| `default`, `debug-log` | 16.06 ms; log lines 226 and 67 |
| `near60-off` | 16.06 ms: the timer ignores the preference |
| `mm-shim` (`webkitgtk-monitor-mm-shim.c` preloaded, GDK reports the kernel's sizes) | 16.06 ms. The log moves to 238 (`drmWaitVBlank failed`) when routing is visible, or stays at 226 when it is not. With `vblank=1`: 13.89 ms |
| `mm-shim-near60-off` | 16.06 ms. With `vblank=1` and visible routing: 6.94 ms |
| `xwayland` | as `default` |
| `force-timer` | 16.06 ms without log lines (the control) |

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

## Activity pass in replay mode

A real catalog does not always hold an Activity that scrolls: on the 120 Hz Mac
`activity-scroll` was skipped as nothing to scroll. The measurement therefore has
two passes in the same display window:

- the **live pass** (no `ZERUS_CLIENT_REPLAY`) measures the real session list
  (`list-scroll`) and, when a live session happens to scroll, `activity-scroll`;
- the **replay pass** measures Activity: the same auto run with
  `ZERUS_CLIENT_REPLAY` set to `client/fixtures/replay.ndjson`, whose session
  `run:000001-run` holds 1100 synthetic events.

Replay never touches real data: read marks go to a separate file and actions stay
blocked without a fake `ZERUS_CLIENT_HGS`. In replay mode the run tries every
session of the catalog (a live run tries 16 at most), reads older pages with
`activity.loadOlder` and keeps the first session that scrolls 2000 px, so the
choice does not depend on list order. The report holds a top-level
`activity_source` object (`replay`, `events`, `scroll_px`, `older_pages`; counts
only), `measure-report.py` prints it in the run details, and the run logs
`activity source: replay, N events, M px, K older pages`. A skipped scroll
scenario says how far it scrolled, for example `nothing to scroll (12 px)`.

Build the binary with the frontend embedded (a plain `cargo build --release`
loads the dev server URL and never starts the run): `npm run build`, then in
`client/src-tauri` `cargo +1.99.1 build --release --locked --features tauri/custom-protocol`.

Linux, headless (never a window on a desktop), from the repository root:

```sh
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_HEADLESS=1 \
  xvfb-run -a -s "-screen 0 1440x900x24" \
  env ZERUS_CLIENT_PERF=auto ZERUS_CLIENT_PERF_OUT="$PWD/replay-linux.json" ZERUS_CLIENT_PERF_LABEL=linux-replay \
      ZERUS_CLIENT_PERF_REPRESENTATIVE=0 ZERUS_CLIENT_REPLAY="$PWD/client/fixtures/replay.ndjson" \
      client/src-tauri/target/release/zerus-client
```

macOS, a bundle of the release build with its own identifier (never the
installed app's or the Phase 2 build's), started by `open` in the console session
like the Qt bench above. `open --env` passes the variables and every path is
absolute:

```sh
client/bench/mac-bundle.sh client/src-tauri/target/release/zerus-client bundles/ZerusPerfReplay.app dev.zerus.next.perfreplay
open -n -W --stdout "$PWD/replay.out" --stderr "$PWD/replay.err" \
  --env ZERUS_CLIENT_PERF=auto --env ZERUS_CLIENT_PERF_OUT="$PWD/replay-mac.json" \
  --env ZERUS_CLIENT_PERF_LABEL=mac-replay --env ZERUS_CLIENT_REPLAY="$PWD/client/fixtures/replay.ndjson" \
  bundles/ZerusPerfReplay.app
```

Run the live pass in the same window with the same bundle and without the
`ZERUS_CLIENT_REPLAY` variable (and another `--env ZERUS_CLIENT_PERF_OUT`). Put
both reports on one page with `measure-report.py`.

## Recorded results

| File | What |
|---|---|
| `results/arch-qt-2026-10-28.json` | Qt harness on Xvfb (not representative) and the running GUI on arch, 60 s |
| `results/arch-webkitgtk-harness-2026-10-28.json` | client scenarios on the synthetic harness, WebKitGTK 2.54.1 on Xvfb (not representative) |
| `results/arch-kde-display-2026-10-28.json` | arch display modes (two 144 Hz outputs at scale 1); real-display runs refused while the session was locked |
| `results/arch-kde-client-main-2026-10-28.json` | the client built from main with PR #385 on the real arch display (144 Hz, unlocked): pacing, session open, and the idle windows as displayed, with Animate working sessions off, with Reduce motion, hidden and minimised |
| `results/arch-kde-webkitgtk-vblank-2026-10-28/` | the real-display WebKitGTK experiment (`zerus-5wmh.14`): one harness run per configuration (default, debug log, near-60 off, mm shim, shim with near-60 off, XWayland, forced timer) and the DRM probe |
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
