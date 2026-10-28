# WebKitGTK display pacing on Linux

Date: 2026-10-28
Beads: `zerus-5wmh.28` (investigation: `zerus-5wmh.14`)

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

- `0001-GTK-match-DRM-connectors-by-name.patch`: GTK 3/GTK 4 name lookup with
  exact-mm compatibility fallback.
- `0002-GLib-use-display-rate-for-timer-fallback.patch`: display-ID-aware timer
  rate, fractional monotonic deadlines and no catch-up burst.
- `0003-GTK3-arm-Wayland-frame-callback-before-GL-swap.patch`: GTK 3.24.52 GL
  end-paint arms the real surface callback before the buffer swap. It also
  applies to gtk-3-24 commit `4ef4903ed50e11bb13a34c5ea07523bd136370a4`.
- The WebKit patches apply with `git apply --check` to WebKit main commit
  `6740c3a3f01a5a450fa7698037385790dfa3aa87` as well as the verified 2.54.1
  source. Main has not been built in this task.
- The client disables the near-60 feature through WebKit's public C API using
  runtime symbol lookup. Its Rust bindings predate that API, and dynamic lookup
  avoids a new hard WebKit 2.42 runtime requirement.
  `ZERUS_CLIENT_PERF_PREFERENCE=default` retains the control behavior.

## Validation

The private WebKitGTK and GTK Release builds and installs succeeded. WebKit's
style checker reported zero errors in all four patched WebKit files. The
production Tauri client completed all seven diagnostics scenarios on private
Xvfb and exited 0, with its real app, network and web processes loading the
private libraries. Xvfb is not evidence of display-rate pacing or the GTK 3
Wayland GL path.

With a GDK shim reporting 144000 mHz, `webkit://gpu` reports `VBlank type Timer`
and `VBlank refresh rate 144Hz`, while the same build's rAF interval remains
about 16.67 ms. This is direct evidence of the downstream presentation gate and
motivates patch 0003; it is not a pass result.

The first real-display attempt was refused before opening a window because the
KDE session was locked and both outputs were in standby. Tests stopped as
required. Final 144 Hz values and any limitations found during qualification
are added here, to the Beads task and to the Russian review page. Raw commands,
exit codes, logs and JSON are under `/w/hgdev/webkit-vblank/evidence/`; rAF
intervals alone are not compositor-phase validation.
