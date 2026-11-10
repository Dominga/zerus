# WebKitGTK display pacing on Linux

Date: 2026-10-28
Beads: `zerus-5wmh.28` (investigation: `zerus-5wmh.14`)

Status: active research, clarified by the owner on 2026-10-28. The WebKit and
CEF investigations continue in parallel; feature work and releases do not wait
for them. The private builds are complete and real-display measurements are
next.

## Recommendation

Fix four independent layers; none replaces the others:

1. **Select a DRM CRTC by connector name, not by exact physical size.** Use
   `gdk_monitor_get_connector()` on GTK 4 and the older public
   `gdk_screen_get_monitor_plug_name()` compatibility API on GTK 3. The latter
   returns the internal GdkMonitor connector, despite its deprecation notice
   recommending the monitor model. Keep exact-mm matching only as the legacy
   fallback when no name is available.
2. **When no usable vblank source exists, run the fallback timer at the selected
   monitor's actual nominal rate.** A 144 Hz display must not silently turn into
   a fixed 60 Hz display merely because `drmWaitVBlank` is unavailable. Use a
   fractional monotonic period and do not emit catch-up bursts after a stall.
3. **Arm the GTK 3 Wayland frame callback before every GL buffer swap.** [GTK
   issue 8461](https://gitlab.gnome.org/GNOME/gtk/-/work_items/8461) is a third
   engine-independent gate: the GL branch clears `pending_commit`, after-paint
   returns without requesting a callback, and the idle frame clock stays at
   16667 µs. Request the callback in the GL end-paint path before
   `eglSwapBuffers()` issues that commit, preserving the existing freeze, thaw
   and presentation-timing code.
4. **Disable `PreferPageRenderingUpdatesNear60FPS` for Zerus's own web view on
   Linux.** WebKit's default otherwise reduces a 144 Hz display link to 72 Hz.
   Keep WebKit's global default and the measurement's `default` control
   unchanged.

The first two changes are WebKit patches, the third is a GTK 3.24.52 patch and
the fourth is a client change. Neither WebKit patch requires
`nvidia_drm.vblank=1`, a driver reload or a reboot. A connector-only patch
cannot fix NVIDIA when that option is off.

The third gate is confirmed by more than source inspection: with a GDK shim
reporting 144000 mHz, `webkit://gpu` shows the patched Timer at 144 Hz while
rAF remains at 60 Hz. The timer therefore works, but presentation completion
still prevents full-rate rendering. Compare stock, WebKit-only and full-stack
builds on the real display. `GDK_GL=disable` is only a diagnostic, not an
acceptable product workaround because it forces CPU copies.

The timer is a pragmatic full-rate fallback, **not** a claim that a
free-running timer is phase-locked to the Wayland compositor. The GTK callback
supplies compositor pacing to the GTK frame clock; a future WebKit
DisplayLink architecture could consume presentation feedback directly rather
than relying on a nominal-rate timer.

## Why exact millimetres fail

`DisplayVBlankMonitorDRM.cpp` in WebKitGTK 2.54.1 and current WebKit main scans
DRM connectors and accepts the first connected connector whose `mmWidth` and
`mmHeight` equal the GdkMonitor values. On the owner's KDE Wayland desktop:

| Output | KWin / GDK (EDID detailed timing descriptor) | Kernel connector (EDID base block, cm x10) |
|---|---:|---:|
| DP-2 | 597x336 mm | 600x340 mm |
| DP-3 | 797x334 mm | 800x330 mm |

Both values are legitimate. This is not a KWin bug and a tolerance would only
hide this instance. Size is not an output identity: identical monitors have the
same size, different models can share rounded sizes, and selecting the first
match can select another output's CRTC.

GTK 3 has no public `GdkMonitor.get_connector`, but it does have
`gdk_screen_get_monitor_plug_name()`. Its implementation calls the internal
`gdk_monitor_get_connector()` and returns a copy. A no-window check on arch
(GDK `3.24.52`, `GdkWaylandDisplay`) returned exactly `DP-2` and `DP-3`.
Therefore WebKit does not need private GDK structures, a second Wayland
`wl_output` listener, EDID files or a preload shim.

The proposed comparison constructs libdrm's normal name from
`drmModeGetConnectorTypeName(connector_type)` plus `connector_type_id` and
compares it with the GDK name. Each DRM device is searched separately, so equal
names on different GPUs do not collide. If a name exists but does not match,
the patch does not fall back to a potentially wrong physical-size match.

## Output and toolkit cases

- **GTK 3 / native Wayland:** the compatibility API supplies the wl_output v4 or
  xdg-output name. Two identical monitor models still have different connector
  names and therefore different CRTCs.
- **GTK 4 / webkitgtk-6.0:** `gdk_monitor_get_connector()` is the direct public
  API and is available from GTK 4.0.
- **X11:** the same GTK 3 compatibility API returns the RandR output name on X11.
  Exact-name matching works when that name agrees with the kernel connector.
- **XWayland:** the same code applies, but WebKit cannot exceed the refresh rate
  that XWayland reports to GDK. On this desktop XWayland previously paced at a
  60 Hz vblank; a timer constructed from a reported 60 Hz remains 60 Hz. Fixing
  XWayland's mode advertisement is outside WebKit.
- **No connector name:** preserve the old exact-size behavior for compatibility.
  Its ambiguity with identical monitors is a known limitation, not a reason to
  introduce a broader size tolerance.
- **WPE:** leave CRTC selection unchanged in this GTK-focused series. WPE
  Platform already has its own screen and sync-observer API (bug
  [294386](https://bugs.webkit.org/show_bug.cgi?id=294386)), but its DRM screen
  also has an exact-mm match and `drmWaitVBlank`, so WPE is not an alternative
  escape path. The GTK timer-rate lookup compiles out on WPE, leaving its
  fallback at 60 Hz; connector and rate fixes for WPEScreen need their own patch.

When a window moves, `WebKitWebViewBase` calls
`WebPageProxy::windowScreenDidChange()` with the new ScreenManager display ID
(`WebKitWebViewBase.cpp:457-470`). Display links are keyed by that ID, so the
new monitor gets its own name lookup and timer rate. A mode change on the same
monitor can leave an existing display link's nominal rate stale; that is also
true of the current DRM implementation and is a separate fix.

EDID manufacturer/model/serial matching was rejected: GDK does not expose a
complete EDID identity, sysfs adds device-mapping and permission problems, and
identical units remain ambiguous. Geometry is also not a universal replacement
because Wayland uses logical positions, scaling and transforms.

## Pacing without DRM vblank

The existing order is sensible: use DRM vblank when it works, otherwise fall
back. NVIDIA's default `nvidia_drm.vblank=N` makes every CRTC here return
`EOPNOTSUPP`, although KWin continues to present normally using page-flip
completion. The connector patch moves WebKit past monitor discovery and exposes
that second failure; it does not remove it.

The timer patch passes the display ID to `DisplayVBlankMonitorTimer::create()`.
On GTK it reads `gdk_monitor_get_refresh_rate()` in millihertz and uses the
same whole-hertz conversion as the DRM monitor. Unknown or unavailable rates
remain `FullSpeedFramesPerSecond` (60). Instead of `sleep_for(milliseconds(
1000 / rate))`, it computes a fractional `steady_clock` period and sleeps until
the next deadline. If the deadline is at least one full period late, the next
phase is reset instead of firing a burst after suspension or a long stall.

Consequences:

- 144 Hz produces a nominal 6.944 ms callback period without NVIDIA options.
- The existing near-60 preference can still intentionally reduce that to 72 Hz.
- The existing start/stop and display-ID lifecycle remains in charge of power.
  No extra thread, surface or permanent 144 Hz wake-up is introduced.
- The timer can still wake between compositor deadlines, so phase jitter and a
  possible extra compositor frame of latency remain. The harness measures
  callback intervals, not scanout proof. VRR and compositor throttling also need
  real presentation feedback, not just a nominal-rate timer.

## Why not implement Wayland pacing in this small patch series?

A Wayland client should normally let the compositor drive presentation.
Chromium's [WaylandFrameManager](https://github.com/chromium/chromium/blob/main/ui/ozone/platform/wayland/host/wayland_frame_manager.cc)
waits for frame callbacks and uses `wp_presentation` feedback for begin-frame
timing. Firefox's [WaylandVsyncSource](https://github.com/mozilla-firefox/firefox/blob/main/widget/gtk/WaylandVsyncSource.cpp)
attaches callbacks to its real Wayland surface and includes hidden-window
fallback logic.

That integration has a surface and presentation lifecycle. WebKit's current
`DisplayVBlankMonitor` is created in the UI process from only a display ID; it
has no web-view surface. The current DisplayLink architecture came from WebKit
bug [261673](https://bugs.webkit.org/show_bug.cgi?id=261673), which removed the
old GTK refresh monitor.

- [`wl_surface.frame`](https://wayland.app/protocols/wayland#wl_surface:request:frame)
  is a drawing hint associated with a surface commit. An invisible helper
  surface is not a reliable global vblank clock and may receive no callbacks
  while occluded.
- [`wp_presentation.feedback`](https://wayland.app/protocols/presentation-time#wp_presentation:request:feedback)
  reports when a particular surface update was displayed, plus the relevant
  output and predicted refresh interval. It is valuable for phase correction,
  but also requires a real presented surface and has discarded/occluded cases.
- [`wp_commit_timing`](https://wayland.app/protocols/commit-timing-v1) sets a
  target time for a surface commit. It does not supply a clock to the UI process.
- [`wp_fifo`](https://wayland.app/protocols/fifo-v1) constrains when successive
  commits become ready. It likewise is not a display-refresh event source and
  explicitly needs another throttling mechanism for off-screen surfaces.

A proper implementation should connect WebKitWebViewBase / DrawingArea /
ThreadedCompositor presentation to the display-refresh monitors, follow the
surface's current output, suspend when hidden and handle monitor moves and
fallback. That can reduce judder and unnecessary work, but it is a larger
cross-process change and needs upstream review. Replacing only the UI-process
timer with an unattached Wayland callback would be less robust, not more.

## Prior art

- WebKit bug [325789](https://bugs.webkit.org/show_bug.cgi?id=325789) already
  reports the exact 597x336 versus 600x340 mm mismatch on KWin with WebKitGTK
  2.54.0. The connector patch should be proposed there, not as a duplicate.
- Bug [199376](https://bugs.webkit.org/show_bug.cgi?id=199376) tracks the broader
  60 fps cap. Its 2026 comments report NVIDIA `EOPNOTSUPP` and, on AMD, the
  additional GTK 3 GL frame-clock cap. The timer patch belongs there.
- [GTK issue 8461](https://gitlab.gnome.org/GNOME/gtk/-/work_items/8461)
  demonstrates the missing GL frame callback on GTK 3 Wayland. Bug 199376
  comment 8 reached 9 ms at 120 Hz only with both the near-60 feature off and
  `GDK_GL=disable`; that is evidence to qualify here, not a fix to copy.
- Bug [314190](https://bugs.webkit.org/show_bug.cgi?id=314190) covers WPE's
  separate unknown-refresh-rate sentinel handling.
- WebKit bug [261673](https://bugs.webkit.org/show_bug.cgi?id=261673) added the
  GTK DisplayLink and the initial DRM-or-timer design.
- Bug [262779](https://bugs.webkit.org/show_bug.cgi?id=262779) handles display
  power-off behavior; bug [266629](https://bugs.webkit.org/show_bug.cgi?id=266629)
  added the fallback when `drmWaitVBlank` fails.
- Bug [276952](https://bugs.webkit.org/show_bug.cgi?id=276952) made DRM device
  discovery continue across nodes. It does not address connector identity.
- Bug [294386](https://bugs.webkit.org/show_bug.cgi?id=294386) added WPE's sync
  observer, which is why this series should not force a GTK design onto WPE.
- NVIDIA's driver source declares vblank support default-disabled; the local
  read-only probe, not WebKit's misleading error string, established
  `EOPNOTSUPP` here. Reports of stalls with `vblank=1` are another reason not
  to make that option a prerequisite.

The connector patch is a focused upstream correction. The timer patch is small
and testable, but upstream should explicitly weigh its open-loop phase and
power trade-offs. Compositor-driven scheduling is the architectural follow-up,
not evidence that these two bugs should remain unfixed.

## Patch and client status

The complete patch set of the private arch stack is in `client/patches/`.
WebKit patches 0001 and 0002 apply to the WebKitGTK 2.54.2 and 2.54.1 releases,
0005 only to 2.54.1, and GTK patches 0003 and 0004 apply to GTK 3.24.52, each
with `patch -p1` from the source root.

- [0001](../../../client/patches/0001-GTK-match-DRM-connectors-by-name.patch):
  GTK 3/GTK 4 name lookup with exact-mm compatibility fallback, in the form
  under upstream review ([325789](https://bugs.webkit.org/show_bug.cgi?id=325789)).
- [0002](../../../client/patches/0002-GLib-use-display-rate-for-timer-fallback.patch):
  display-ID-aware timer rate, fractional monotonic deadlines and no catch-up
  burst.
- [0003](../../../client/patches/0003-GTK3-arm-Wayland-frame-callback-before-GL-swap.patch):
  GTK 3.24.52 GL end-paint arms the real surface callback before the buffer
  swap. It also applies to gtk-3-24 commit
  `4ef4903ed50e11bb13a34c5ea07523bd136370a4`.
- [The companion frame-clock correction 0004](../../../client/patches/0004-GDK-complete-paint-before-thaw.patch)
  completes the paint phase before a backend freeze resumes. Otherwise a busy
  producer can repeat paint after every thaw, starving layout and after-paint
  while a resized window keeps its old child allocation. Apply it to the same
  GTK source after patch 0003. The private-Xvfb regression is
  `client/scripts/test-gtk-frame-clock.py`; the native Wayland resize and pacing
  regression is `client/scripts/window-resize-smoke.py` with the `resize_fixture`
  example. Both require an explicitly selected private build.
- [0005](../../../client/patches/0005-Skia-keep-ended-animation-values.patch)
  backports the WebKitGTK 2.54.2 `SkiaCompositingLayer` change. In 2.54.1 the
  default Skia compositor painted a layer's own opacity, transform and filter
  once an animation had ended on the compositor but before the web process
  committed the final style, so every finite CSS entrance presented one frame
  near its start values at its end. Its regressions are
  `client/scripts/task-view-flicker-smoke.html?entrance` and
  `client/scripts/entrance-survey-smoke.html` under
  `client/scripts/menu-smoke.py --paint-column 100`. 2.54.2 already contains
  the change (`patch --dry-run` reports 0005 as previously applied), so a
  2.54.2 build omits it.
- [0006](../../../client/patches/0006-GTK3-commit-resized-configures-with-their-paint.patch)
  keeps a size-changing Wayland configure acknowledgement with the resized
  paint. After thaw, the previous frame's after-paint can otherwise commit the
  old geometry before the new layout runs. KWin consumes that acknowledgement,
  then centers the later geometry change as a spontaneous resize: maximize can
  straddle outputs and restore can move the normal window. State-only
  configures still request their after-paint commit. Apply this to GTK after
  0003 and 0004; it changes neither Tauri geometry persistence nor X11 behavior.
  `client/scripts/test-wayland-maximize.py` checks the actual production resize
  fixture in a private, actively compositing KWin VirtualBackend, with two
  outputs and a private bus and home. It verifies the selected GDK mapping,
  maximized output bounds and the exact restored normal rectangle. Its optional
  uncorrected GDK control must reproduce the failure. It never uses the owner's
  display or installed GUI.
- WebKit patches 0001 and 0002 apply with `git apply --check` to WebKit main
  commit `6740c3a3f01a5a450fa7698037385790dfa3aa87` and to the verified 2.54.1
  source, and to the verified 2.54.2 source without offset or fuzz. Main has
  not been built in this task.
- The client disables the near-60 feature through WebKit's public C API using
  runtime symbol lookup. Its Rust bindings predate that API, and dynamic lookup
  avoids a new hard WebKit 2.42 runtime requirement.
  `ZERUS_CLIENT_PERF_PREFERENCE=default` retains the control behavior.

### Building the private prefix

`SOURCES`, `BUILD` and `PREFIX` are private absolute directories, and `ZERUS`
is a checkout of this repository. `PREFIX` is the final directory the launcher
will select (see below). The WebKitGTK release is `webkitgtk-2.54.2.tar.xz`
from webkitgtk.org, SHA-256
`a76345b391bef6211fb5ab6cfab2e86b285d18cda17cbb9ade595f1252b86065`. A 2.54.1
build (`ea0bbb02dbdbc596874a4e7ad35b66645b3e0a232bd0e4081de5ed92eb0a397d`)
also applies 0005.

```sh
cd "$SOURCES" && tar -xJf webkitgtk-2.54.2.tar.xz && cd webkitgtk-2.54.2
for patch in 0001-GTK-match-DRM-connectors-by-name 0002-GLib-use-display-rate-for-timer-fallback; do
  patch -p1 < "$ZERUS/client/patches/$patch.patch"
done
cmake -S . -B "$BUILD/webkit" -G Ninja -DPORT=GTK -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_INSTALL_LIBDIR=lib -DUSE_GTK4=OFF \
  -DENABLE_TOOLS=OFF -DENABLE_TESTING=OFF -DENABLE_MINIBROWSER=OFF -DENABLE_WEBDRIVER=OFF \
  -DENABLE_DOCUMENTATION=OFF -DENABLE_INTROSPECTION=OFF -DENABLE_BUBBLEWRAP_SANDBOX=OFF \
  -DENABLE_SPEECH_SYNTHESIS=OFF -DUSE_LIBBACKTRACE=OFF
cmake --build "$BUILD/webkit" && cmake --install "$BUILD/webkit"

cd "$SOURCES" && git clone --depth 1 --branch 3.24.52 https://gitlab.gnome.org/GNOME/gtk.git gtk-3.24.52
cd gtk-3.24.52
patch -p1 < "$ZERUS/client/patches/0003-GTK3-arm-Wayland-frame-callback-before-GL-swap.patch"
patch -p1 < "$ZERUS/client/patches/0004-GDK-complete-paint-before-thaw.patch"
patch -p1 < "$ZERUS/client/patches/0006-GTK3-commit-resized-configures-with-their-paint.patch"
meson setup "$BUILD/gtk" -Dbuildtype=release -Dprefix="$PREFIX/gtk" -Dlibdir=lib \
  -Dx11_backend=true -Dwayland_backend=true -Dbroadway_backend=false -Dintrospection=false \
  -Ddemos=false -Dexamples=false -Dtests=false -Dinstalled_tests=false -Dgtk_doc=false -Dman=false
meson compile -C "$BUILD/gtk" && meson install -C "$BUILD/gtk"
```

To check maximize/restore without using the owner's display, build the existing
isolated fixture and use the selected private libraries below. `CONTROL` is an
otherwise equivalent GTK prefix without 0006; omit its argument when no control
is available. The control must fail at least one geometry check, while the
corrected library must pass both. The output directory must be fresh.

```sh
CARGO_TARGET_DIR="$BUILD/client" cargo build --locked \
  --manifest-path "$ZERUS/client/src-tauri/Cargo.toml" \
  --example resize_fixture --features tauri/custom-protocol
python3 "$ZERUS/client/scripts/test-wayland-maximize.py" \
  --fixture "$BUILD/client/debug/examples/resize_fixture" \
  --webkit-prefix "$PREFIX" --lib-dir "$PREFIX/gtk/lib" \
  --control-lib-dir "$CONTROL/gtk/lib" --output "$BUILD/maximize-check"
```

The result has `lib/`, `libexec/webkit2gtk-4.1/` and the `gtk/` override that
`packaging/desktop/linux-launcher.py` requires. It is selected as described in
[installation](../../installation.md). A WebKit-only prefix without `gtk/` uses
system GTK and stays at 60 Hz.

The prefix is not relocatable. `LD_LIBRARY_PATH` selects only its libraries.
WebKit compiles its install directories in: the helper processes
(`PKGLIBEXECDIR`), the injected bundle (`PKGLIBDIR`) and its translations.
Release builds have `ENABLE_DEVELOPER_MODE` off, and WebKit then ignores
`WEBKIT_EXEC_PATH`, so the launcher's value does not choose the helpers;
`WEBKIT_INJECTED_BUNDLE_PATH` would move only the bundle.
GTK likewise reads its settings, input-method cache and translations from its
own `-Dprefix`. A copied or renamed prefix therefore keeps running
`WebKitWebProcess`, `WebKitNetworkProcess` and the injected bundle from the tree
it was installed to. On arch this was seen with both a copied 2.54.2 stage and
the 2.54.1 prefixes. Install with `cmake --install` and `meson install` into
the final `PREFIX` (or with `DESTDIR` and move the tree to exactly that path).
A `DESTDIR` install skips meson's post-install `gio-querymodules` and
`glib-compile-schemas` steps; the arch prefixes have run without those caches.
Never copy a library from the build tree: its build-tree `DT_RPATH` is searched
before `LD_LIBRARY_PATH`, and the installation step removes it.

Keep the previously selected prefix as the rollback, together with every tree
its compiled paths name. The arch 2.54.1 prefixes depend on two trees outside
themselves. They run their helper processes and injected bundle from the 2.54.1
base install their libraries came from, and read GTK data from that install's
`gtk/`. The 0005 library of the Skia rollback was copied from the 2.54.1 build
tree and keeps a `DT_RPATH` into it; a probe's UI process loaded JavaScriptCore
from there, byte-identical to the prefix copy. Keep the base install and the
2.54.1 build tree for as long as either prefix is the rollback.

Qualify a new prefix before selecting it:

- No library in `$PREFIX/lib` or `$PREFIX/gtk/lib` has an `RPATH` or `RUNPATH`
  (`readelf -d`). With `LD_LIBRARY_PATH` and `WEBKIT_EXEC_PATH` set as
  `packaging/desktop/linux-launcher.py` sets them, a private-Xvfb `menu-smoke.py` window run
  under `strace -f -e trace=execve,openat` execs and opens private files only
  under `$PREFIX`. The `/proc/<pid>/exe` of its `WebKitWebProcess` and
  `WebKitNetworkProcess`, and the injected bundle in the web process's maps,
  are under `$PREFIX` too.
- `client/scripts/test-gtk-frame-clock.py` in its private Xvfb.
- `client/scripts/window-resize-smoke.py --webkit-prefix "$PREFIX" --min-fps 130
  --timer-burst` on the physical display, holding the display lock as its usage
  text describes. On the arch 144 Hz display the 2.54.1 set with 0005 measured
  142.5 Hz rAF and 143 Hz GTK paint and after-paint. The 2.54.2 set measured
  143 Hz for all three. Both applied every resize transition.
- `task-view-flicker-smoke.html?entrance` and `entrance-survey-smoke.html` under
  `menu-smoke.py --paint-column 100`, with the same launcher variables. No
  presented frame may dim a finished entrance.

## Validation

The private WebKitGTK and GTK Release builds and installs succeeded. WebKit's
style checker reported zero errors in all four patched WebKit files. The
production Tauri client completed all seven diagnostics scenarios on private
Xvfb and exited 0, with its real app, network and web processes loading the
private libraries. Xvfb is not evidence of display-rate pacing or the GTK 3
Wayland GL path.

An isolated `kwin_wayland --virtual` session with a nominal 144 Hz output
(current mode 143.606 Hz) exercised the EGL Wayland path with
`__NV_DISABLE_EXPLICIT_SYNC=1`. WebKit-only list scrolling averaged 16.67 ms;
with patch 0003 it averaged 7.03 ms (median 7.00 ms). Transform and light rAF
runs were also near 7 ms. The software virtual compositor introduced enough
jitter for some intervals to fail the strict harness criteria, so this is
callback-path evidence, not a physical-display qualification.

A physical-display WebKit-only run then confirmed the first two fixes: its log
reaches `drmWaitVBlank` after connector-name matching, and light rAF and
transform animation meet the 144 Hz harness criteria at 6.94 ms. List scrolling
remains at 16.67 ms without the GTK patch. `nvidia_drm.vblank=N` was unchanged.

With a GDK shim reporting 144000 mHz, `webkit://gpu` reports `VBlank type Timer`
and `VBlank refresh rate 144Hz`, while the same build's rAF interval remains
about 16.67 ms. This is direct evidence of the downstream presentation gate and
motivates patch 0003; it is not a pass result.

Earlier real-display attempts were refused before opening a window because
the KDE session was locked and both outputs were in standby. The owner then
clarified that the research continues in parallel with feature work and
releases. Raw commands, exit codes, logs and JSON are preserved in the task's
WebKit evidence directory; final measurements are added after the display runs.
rAF intervals alone are not compositor-phase validation.
