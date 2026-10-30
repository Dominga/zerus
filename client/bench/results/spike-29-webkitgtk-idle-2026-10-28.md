# Spike zerus-5wmh.29: why WebKitGTK idles at 20-30% of a core in the app process

Arch, KDE Plasma Wayland, NVIDIA 615.78.08 (open kernel module), two outputs at 144 Hz, scale 1.
webkit2gtk-4.1 2.54.1, gtk3 3.24.52. Client: the release binary built from main 0a6fa30b.
Every figure is **measured** (on the real display, inside the shared display lock) unless it is
marked **estimated** or **inferred**. CPU is percent of one core over the client's own 10 s idle
windows. 22 runs between 10:27 and 13:29 (2026-10-28); 6 of them were DMA-BUF variants that
died at start. No window is left open and the spike worktree is removed.

## Answer first

1. **A cheap win exists, but it is a renderer setting, not CSS, and the global setting blocks it.**
   The client runs on WebKitGTK's shared-memory (CPU) renderer only because `/etc/environment`
   sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` for the whole desktop (owner's file, 2024-11-04; left
   untouched). With the DMA-BUF renderer plus NVIDIA's `__NV_DISABLE_EXPLICIT_SYNC=1`, for this
   process only, the same live page with the same smooth motion at 60 fps measured:

   | | total (client tree) | app | web | kwin (outside the tree) | runs |
   |---|---|---|---|---|---|
   | Shared memory, as displayed (zl-spin, zl-clock, count-pulse x5, halo x2, pulse x3) | 80.6% | 28.9% | 41.8% | 32.1% | base-d |
   | DMA-BUF + `__NV_DISABLE_EXPLICIT_SYNC=1`, same animation set | 17.7% | 5.5% | 5.5% | 9.9% | on-nvsync |
   | Shared memory, as displayed (count-pulse x4, halo x1, pulse x3) | 76.7% | 31.7% | 34.9% | 32.3% | base-c |
   | DMA-BUF + explicit sync off, repeat (count-pulse x6, halo x3, pulse x3) | 20.5% | 6.5% | 4.8% | 7.2% | on-nvsync-b |

   Exact change: in the client's own environment before GTK starts,
   `WEBKIT_DISABLE_DMABUF_RENDERER=0` (WebKit treats "0" as not disabled,
   `UIProcess/gtk/AcceleratedBackingStore.cpp:86-88`) and `__NV_DISABLE_EXPLICIT_SYNC=1`.
   Without the second variable the DMA-BUF renderer dies within a second with
   `Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display` (6 of 6 runs). A pixel
   readback of GTK's back buffer in on-nvsync-b showed the app's dark UI, not a blank surface
   (details below); what reached the screen was not captured. Whether `auto` may override the
   global variable is the policy question in the last section; I recommend it may not.
2. **No CSS-only win keeps the approved motion.** Every visible change anywhere in the page costs
   one full-window repaint in the app process, whatever its size. Stepped pulses cut the cost only
   when every running indicator steps slowly and in phase: steps(4) on the pulses measured app
   3.3% (prop, after_hide window) instead of 26.9% (base-shim), but that is visibly stepped motion
   (8 levels per 1.6 s cycle; **inferred**, no visual check). steps(12) on every working
   indicator gave no reduction once the staggered typing dots ran (all12, after_hide window,
   no Zerus mark: 58 full-window paints/s, app 25.1%). `will-change` layers
   left the app process unchanged (layers: app 27.0%).
3. **Why the app's cost scales with the frame rate:** per frame, WebKitGTK's GTK3 backing store
   asks GTK to redraw the whole web view (`gtk_widget_queue_draw`, the frame's damage rectangles
   are ignored), so GTK repaints all 1440x900 pixels. On the shared-memory path one repaint costs
   about 4.3-5.1 ms of app CPU (two thirds of it kernel time: a fresh 5 MB Wayland shm buffer is
   faulted in every frame) plus about 4 ms in KWin, which uploads the whole buffer (**estimated**
   from KWin's CPU minus its level with the window hidden). App CPU is
   frames per second times that: 62 fps gave 26.9-31.7%, 136 fps gave 54.4% (on-shm). On the
   DMA-BUF/GL path a repaint costs about 0.9-1.1 ms (5.5-6.5% at 60 fps).

## Where the app-process cycles go (shared-memory path, the owner's default)

Measured with the paint-counting shim and `/proc` per-thread samples:
- 62 GTK paints a second, every one covering the full 1440x900 window (base-shim, base-c, base-d),
  against 0.2 a second with Animate working sessions off. Six small pulses therefore cost
  80 Mpx/s of CPU compositing.
- 1266 minor page faults per paint (78 650/s at 62.1 paints/s, base-c): exactly one fresh
  1440x900x4 buffer (1265.6 pages). With an idle window the fault rate is about 1 270/s at 1 paint/s.
- User time is 27-33% of the app's CPU (steps 4.8 of 17.8; base-c 9.9 of 31.7; base-d 9.4 of 28.9);
  the rest is kernel time.
- The app's main thread carries it all (25.9% of 26.9% in base-shim); tokio, hgs-watch and the
  other client threads are below 0.2% together. None of it is in the client's Rust or TypeScript.

Profile of the app process (run `profile`, perf `cycles:u` at 199 Hz with DWARF call graphs,
9 s of idle as displayed; user space only because `perf_event_paranoid` is 2). App samples are
36.3% of all samples in the window (WebKitWebProcess 62.2%):

| Inclusive share of app user cycles | Frames |
|---|---|
| 82% | GTK frame clock paint: `gdk_frame_clock_paint_idle` > `gdk_window_process_updates_internal` > `gtk_widget_render` |
| 72% | `cairo_fill` > `_cairo_spans_compositor_paint` > pixman |
| 36% | `sse2_composite_over_8888_8888` (pixman): WebKit's `AcceleratedBackingStore::Buffer::paint` blending the shm frame OVER the window (`cairo_set_source_surface` + `cairo_fill`, `AcceleratedBackingStore.cpp:274-299`) |
| 27% | `sse2_fill` (pixman): GtkWindow's CSS background (`gtk_window_draw` > `gtk_css_style_render_background`, 22% inclusive) and GDK clearing the paint region (`gdk_window_begin_paint_internal` > `gdk_window_clear_backing_region`, 16% inclusive) |
| under 3% each | GLib main loop, wayland-client dispatch, `__vdso_clock_gettime`, GDK Wayland buffer handling (`_gdk_wayland_display_create_shm_surface`, `gdk_wayland_cairo_surface_destroy`, `read_back_cairo_surface` appear) |

The kernel two thirds are not in a user-space profile; the fault count above and the GTK source
explain them (**inferred** from source plus the fault count): GTK3's Wayland backend
(`gdk/wayland/gdkwindow-wayland.c:904-958, 966-1001`) must allocate a new shm staging buffer
whenever the compositor still holds the last committed one, and KWin holds a surface's current
buffer until the next commit, so each frame gets a new memfd buffer whose 1 265 pages fault in.

The web process (13% with six pulses, 35-61% once the typing and status dots also run) spends 94%
of its samples in libwebkit2gtk (Skia CPU rasterisation, all on its main thread). Arch has no
debuginfod entry for webkit2gtk-4.1 2.54.1-1, so its frames stay addresses.

Answers to the coordinator's comparisons:
- **CEF session's run** (cited, not mine): WebKitGTK 50.4% as displayed (app 26%, web 17%,
  hgs 7%), 7.9% with Animate working sessions off; CEF 22.2% and 10.7%. My shared-memory runs
  agree: app 26.9-31.7% whatever the number of animations (6 to 15), because the cost is per frame
  and every frame is a full-window repaint; web grows with what animates. So yes, the app-process
  time is WebKitGTK's presentation path, specifically the GTK3 shared-memory swapchain
  (`BufferSHM` frames painted with cairo after a whole-view `gtk_widget_queue_draw`), GTK3's
  Wayland shm buffers and KWin's uploads. It is not the DMA-BUF swapchain (disabled here) and not
  our code. Frame callbacks matter only as the pacer: on this path GTK paints as often as WebKit
  sends frames (60 a second here, 136 in on-shm).
- **Kimi's runs** (cited, not mine): the patched stack at 144 Hz idles at 22% (app 16%) under
  shared memory, and at 5.3% (app 3.2%, web 2.1%) under DMA-BUF with `__NV_DISABLE_EXPLICIT_SYNC=1`;
  stock 60 Hz animate-off 4.7%; Mac about 3%. App 16% at 144 fps is 1.1 ms a frame, a quarter of
  my stock shared-memory cost per frame, so their patch evidently removes part of the per-frame
  work (**inferred**; I did not read their patch). Their DMA-BUF app 3.2% at 144 Hz is lower per
  frame than my stock GL path (0.9-1.1 ms); unexplained with my data. The Mac idles at 3% because
  WebKit there hands accelerated animations to Core Animation, which animates them without
  repainting in the app (**inferred**, not measured here).
- **Frame times for zerus-5wmh.36** (WebKit's 60 Hz pacing, my runs, mean/p95/max ms):
  activity-scroll 17.2/25/41, 18.6/23/27, 20.4/28/34, 19.6/27/36 on shared memory (base-shim,
  base-a, base-c, base-d) against 16.1/18/21 and 16.1/19/25 on DMA-BUF (on-nvsync, on-nvsync-b);
  list-scroll 17-18 p95 on both. I have no 144 Hz frame times.

## The first "idle" window against the later ones

It is a real recurring cost, not settling, GC or harness timing. The later windows of
`ZERUS_CLIENT_PERF=auto` are different states by design: Animate working sessions off, Reduce
motion, hidden, minimised. The one later window that returns to "as displayed" (idle after hide)
reads the same as the first: base-shim 47.6 then 41.3, base-c 76.7 then 78.4, base-d 80.6 then
79.4, on-nvsync 17.7 then 22.4. Per second inside the first window (base-shim) the app holds
24-30% at 60-65 paints/s from its first second to its last; JavaScriptCore's heap threads
(HeapHelper, Collector) show 0.0%. The 10 s settle before it does contain a burst (app up to 45%,
96-123 paints/s at 2.6-5.6 s after "scenarios done"), which ends 4 s before the window opens.
"Idle after minimize" reads low only because the window is still minimised (next section).

## The minimise result

On KDE Wayland, Tauri's `unminimize()` does not restore a minimised GTK3 window (xdg-shell has
no unminimize request; GDK's deiconify only shows an already mapped window) and `is_minimized()`
reports false. A read-only KWin script reported `minimized=true` during "window minimize" and
still during "idle after minimize" in every Wayland run. On the shared-memory path KWin then sends
no frame callbacks, GTK stops painting (0 paints/s), WebKit waits for FrameDone and CPU drops to
about 6% while the page still counts as visible with its animations "running". On XWayland the
window does come back (x11-b: idle after minimize 51.4%, 60 paints/s). On the DMA-BUF/GL path the
minimised window keeps painting 60 times a second (on-nvsync: 18.5%, on-nvsync-b: 21.9%).

## Results per variant

`pre`/`post`: before or after the Dev 1503 build and Qt GUI restart (installed hgs 457eb710...,
06:39, then 72cab1b8..., 11:33). The live page changed between runs (how many sessions work, and
whether the Zerus mark spins for attention), so compare runs with the same animation set.
Paints are GTK paints a second in the app process; all were full-window. kwin and Xwayland are
outside the client tree. n/a: not sampled in that run (sampler fields were added during the batch).

| run | hgs | variant | window | animations running | total | app | web | hgs children | GTK paints/s (all full-window) | app user | app faults/s | kwin | Xwayland |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| base-shim | pre | SHM (owner default), shim | displayed | count-pulse x4, halo x2 | 47.6 | 26.9 | 13.3 | 7.4 | 62.2 | n/a | n/a | n/a | n/a |
| base-shim | pre | SHM (owner default), shim | animate_off | none | 7.9 | 0.8 | 0.7 | 6.4 | 0.2 | n/a | n/a | n/a | n/a |
| base-shim | pre | SHM (owner default), shim | after_hide | count-pulse x4, halo x2 | 41.3 | 23.4 | 11.6 | 6.3 | 62.3 | n/a | n/a | n/a | n/a |
| profile | pre | SHM, perf attached (perturbed) | displayed | count-pulse x3, halo x1 | 56.6 | 32.3 | 17.6 | 6.8 | 61.9 | n/a | n/a | n/a | n/a |
| profile | pre | SHM, perf attached (perturbed) | animate_off | none | 7.7 | 0.8 | 0.5 | 6.4 | 0.1 | n/a | n/a | n/a | n/a |
| profile | pre | SHM, perf attached (perturbed) | after_hide | count-pulse x3, halo x1 | 53.4 | 30.0 | 16.2 | 7.2 | 62.4 | n/a | n/a | n/a | n/a |
| steps | pre | SHM, count-pulse+halo steps(4) | displayed | zl-spin x1, zl-clock x1, count-pulse x4, halo x2 | 32.9 | 17.8 | 7.7 | 7.4 | 38.8 | 4.8 | 49230.3 | n/a | n/a |
| steps | pre | SHM, count-pulse+halo steps(4) | animate_off | zl-spin x1, zl-clock x1 | 27.9 | 15.6 | 5.0 | 7.4 | 36.7 | 4.7 | 46418.0 | n/a | n/a |
| steps | pre | SHM, count-pulse+halo steps(4) | after_hide | zl-spin x1, zl-clock x1, count-pulse x4, halo x2 | 32.4 | 16.3 | 8.9 | 7.3 | 38.6 | 5.7 | 48818.7 | n/a | n/a |
| prop | pre | SHM, steps(4) via @property | displayed | zl-spin x1, zl-clock x1, spike-count-pulse x5, spike-halo x3 | 20.9 | 9.0 | 5.9 | 6.1 | 20.3 | 2.7 | 25742.5 | 10.9 | 0.0 |
| prop | pre | SHM, steps(4) via @property | animate_off | none | 8.7 | 1.8 | 1.2 | 5.7 | 2.8 | 0.7 | 3523.1 | 4.9 | 0.0 |
| prop | pre | SHM, steps(4) via @property | after_hide | spike-count-pulse x5, spike-halo x3 | 11.2 | 3.3 | 2.7 | 5.2 | 6.1 | 0.9 | 7740.4 | 5.9 | 0.0 |
| on-shm | pre | DMA-BUF allowed + FORCE_SHM | displayed | zl-spin x1, zl-clock x1, count-pulse x4, halo x2 | 96.1 | 54.4 | 34.8 | 6.9 | 135.8 | 15.8 | 171800.6 | 53.2 | 0.0 |
| on-shm | pre | DMA-BUF allowed + FORCE_SHM | animate_off | zl-spin x1, zl-clock x1 | 95.5 | 52.8 | 34.6 | 8.1 | 139.0 | 14.7 | 175916.7 | 50.9 | 0.0 |
| on-shm | pre | DMA-BUF allowed + FORCE_SHM | after_hide | zl-spin x1, zl-clock x1, count-pulse x4, halo x2 | 92.8 | 52.4 | 34.9 | 5.6 | 137.7 | 14.3 | 174234.2 | 52.0 | 0.1 |
| x11 | post | SHM, XWayland | displayed | zl-spin x1, zl-clock x1, count-pulse x6, halo x4, pulse x3 | 63.7 | 6.1 | 50.1 | 7.5 | 60.2 | 5.0 | 12.5 | 11.0 | 8.6 |
| x11 | post | SHM, XWayland | animate_off | zl-spin x1, zl-clock x1 | 13.3 | 3.1 | 4.0 | 6.2 | 38.6 | 2.7 | 7.2 | 8.6 | 4.4 |
| x11 | post | SHM, XWayland | after_hide | zl-spin x1, zl-clock x1, count-pulse x6, halo x4, pulse x3 | 59.3 | 5.9 | 46.6 | 6.9 | 60.0 | 5.0 | 2.1 | 9.2 | 8.3 |
| base-a | post | SHM, no shim | displayed | zl-spin x1, zl-clock x1, count-pulse x6, halo x4, pulse x3 | 94.5 | 25.3 | 60.9 | 8.3 | n/a | 7.8 | 78778.6 | 28.0 | 0.0 |
| base-a | post | SHM, no shim | animate_off | zl-spin x1, zl-clock x1 | 44.4 | 22.7 | 11.8 | 9.9 | n/a | 6.2 | 65424.7 | 25.2 | 0.1 |
| base-a | post | SHM, no shim | after_hide | zl-spin x1, zl-clock x1, count-pulse x5, halo x3, pulse x3 | 84.2 | 26.4 | 50.5 | 7.3 | n/a | 7.7 | 78765.6 | 28.4 | 0.0 |
| steps12 | post | SHM, count-pulse+halo steps(12) | displayed | zl-spin x1, zl-clock x1, count-pulse x4, halo x2 | 35.2 | 17.3 | 11.5 | 6.4 | 43.1 | 5.4 | 54709.3 | 19.1 | 0.0 |
| steps12 | post | SHM, count-pulse+halo steps(12) | animate_off | zl-spin x1, zl-clock x1 | 25.1 | 14.4 | 5.0 | 5.8 | 37.3 | 4.6 | 47255.2 | 17.4 | 0.0 |
| steps12 | post | SHM, count-pulse+halo steps(12) | after_hide | zl-spin x1, zl-clock x1, count-pulse x3, halo x1 | 35.2 | 18.7 | 8.9 | 7.6 | 44.7 | 6.2 | 56648.8 | 24.0 | 0.0 |
| all4 | post | SHM, all indicators steps(4) | displayed | zl-spin x1, zl-clock x1, count-pulse x3, halo x1 | 29.9 | 16.2 | 6.8 | 6.9 | 39.0 | 4.6 | 49371.1 | 17.4 | 0.0 |
| all4 | post | SHM, all indicators steps(4) | animate_off | zl-spin x1, zl-clock x1 | 30.4 | 17.3 | 7.4 | 5.7 | 36.6 | 6.9 | 46276.5 | 24.6 | 0.0 |
| all4 | post | SHM, all indicators steps(4) | after_hide | zl-spin x1, zl-clock x1, count-pulse x3, halo x1 | 37.9 | 19.7 | 10.5 | 7.8 | 38.6 | 7.2 | 48808.2 | 26.2 | 0.1 |
| all12 | post | SHM, all indicators steps(12) | displayed | zl-spin x1, zl-clock x1, count-pulse x4, halo x1, pulse x3 | 54.5 | 24.9 | 21.1 | 8.5 | 60.2 | 7.9 | 76268.1 | 26.4 | 0.1 |
| all12 | post | SHM, all indicators steps(12) | animate_off | zl-spin x1, zl-clock x1 | 29.0 | 16.5 | 5.1 | 7.4 | 34.5 | 4.4 | 43674.8 | 17.4 | 0.0 |
| all12 | post | SHM, all indicators steps(12) | after_hide | zl-spin x1, zl-clock x1, count-pulse x4, halo x1, pulse x3 | 59.2 | 25.1 | 25.4 | 8.7 | 58.1 | 7.1 | 73467.5 | 26.7 | 0.0 |
| base-c | post | SHM, shim | displayed | count-pulse x4, halo x1, pulse x3 | 76.7 | 31.7 | 34.9 | 10.2 | 62.1 | 9.9 | 78650.5 | 32.3 | 0.0 |
| base-c | post | SHM, shim | animate_off | none | 24.7 | 9.7 | 5.0 | 10.1 | 16.9 | 3.3 | 21341.7 | 16.3 | 0.2 |
| base-c | post | SHM, shim | after_hide | zl-spin x1, zl-clock x1, count-pulse x4, halo x1, pulse x3 | 78.4 | 33.0 | 36.2 | 9.3 | 62.1 | 10.9 | 78625.0 | 36.3 | 0.1 |
| on-nvsync | post | DMA-BUF + __NV_DISABLE_EXPLICIT_SYNC=1 | displayed | zl-spin x1, zl-clock x1, count-pulse x5, halo x2, pulse x3 | 17.7 | 5.5 | 5.5 | 6.8 | 59.9 | 4.6 | 16.2 | 9.9 | 0.0 |
| on-nvsync | post | DMA-BUF + __NV_DISABLE_EXPLICIT_SYNC=1 | animate_off | zl-spin x1, zl-clock x1 | 20.5 | 8.0 | 4.7 | 7.9 | 60.0 | 6.9 | 1.1 | 11.4 | 0.0 |
| on-nvsync | post | DMA-BUF + __NV_DISABLE_EXPLICIT_SYNC=1 | after_hide | zl-spin x1, zl-clock x1, count-pulse x5, halo x2, pulse x3 | 22.4 | 7.7 | 6.0 | 8.8 | 60.0 | 6.4 | 3.0 | 11.4 | 0.0 |
| x11-b | post | SHM, XWayland | displayed | zl-spin x1, zl-clock x1, count-pulse x5, halo x2, pulse x3 | 53.0 | 7.0 | 37.4 | 8.6 | 59.9 | 5.9 | 1.6 | 14.4 | 10.2 |
| x11-b | post | SHM, XWayland | animate_off | zl-spin x1, zl-clock x1 | 18.3 | 4.2 | 6.5 | 7.7 | 37.5 | 3.4 | 2.6 | 7.6 | 5.7 |
| x11-b | post | SHM, XWayland | after_hide | zl-spin x1, zl-clock x1, count-pulse x5, halo x2, pulse x3 | 52.5 | 7.2 | 36.7 | 8.7 | 60.2 | 6.2 | 1.6 | 12.0 | 10.2 |
| layers | post | SHM, will-change on indicators | displayed | count-pulse x5, halo x2, pulse x3 | 61.8 | 27.0 | 26.9 | 7.9 | 62.3 | 8.0 | 78776.4 | 26.1 | 0.0 |
| layers | post | SHM, will-change on indicators | animate_off | none | 8.5 | 1.0 | 0.8 | 6.7 | 1.0 | 0.2 | 1271.6 | 5.3 | 0.1 |
| layers | post | SHM, will-change on indicators | after_hide | count-pulse x5, halo x2, pulse x3 | 65.6 | 27.4 | 29.1 | 9.1 | 62.2 | 7.9 | 78799.4 | 27.6 | 0.0 |
| base-d | post | SHM, shim | displayed | zl-spin x1, zl-clock x1, count-pulse x5, halo x2, pulse x3 | 80.6 | 28.9 | 41.8 | 10.0 | 62.2 | 9.4 | 78638.9 | 32.1 | 0.1 |
| base-d | post | SHM, shim | animate_off | zl-spin x1, zl-clock x1 | 31.0 | 16.8 | 6.3 | 7.9 | 37.4 | 4.6 | 47409.5 | 19.9 | 0.0 |
| base-d | post | SHM, shim | after_hide | zl-spin x1, zl-clock x1, count-pulse x5, halo x2, pulse x3 | 79.4 | 30.0 | 38.6 | 10.9 | 62.2 | 9.1 | 78772.4 | 30.8 | 0.0 |
| on-nvsync-b | post | DMA-BUF + __NV_DISABLE_EXPLICIT_SYNC=1, readback | displayed | count-pulse x6, halo x3, pulse x3 | 20.5 | 6.5 | 4.8 | 9.3 | 60.0 | 5.2 | 11.5 | 7.2 | 0.0 |
| on-nvsync-b | post | DMA-BUF + __NV_DISABLE_EXPLICIT_SYNC=1, readback | animate_off | none | 10.9 | 1.1 | 2.2 | 7.6 | 1.3 | 0.8 | 13.0 | 5.0 | 0.0 |
| on-nvsync-b | post | DMA-BUF + __NV_DISABLE_EXPLICIT_SYNC=1, readback | after_hide | count-pulse x6, halo x3, pulse x3 | 21.1 | 6.9 | 5.3 | 9.0 | 59.7 | 5.9 | 46.1 | 8.6 | 0.1 |

Runs without an idle window (died at start, one log line each,
`Error 71 (Protocol error) dispatching to Wayland display`): dmabuf-on
(`WEBKIT_DISABLE_DMABUF_RENDERER` unset), on-xr24 (+ `WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT=XR24`),
on-compositing-off (+ `WEBKIT_DISABLE_COMPOSITING_MODE=1`), on-steps, on-prop, on-profile.
`WEBKIT_DISABLE_COMPOSITING_MODE=1` on top of the default changes nothing: in 2.54 GTK the web
view always paints through the AcceleratedBackingStore; without hardware acceleration the
threaded compositor renders with Skia CPU into shared memory (source, `AcceleratedSurface.cpp:590-600`).

What the variants show:
- **DMA-BUF allowed + `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`** (on-shm, the shared-memory
  transport with GPU compositing) is a trap: accelerated animations then render on every frame
  callback, 136 full-window paints/s, total 96.1% (app 54.4%, web 34.8%, kwin 53.2%), even with
  only the Zerus mark at rest (139 paints/s). WebKit requests a new composition on every frame
  while any layer animates, whatever the timing function
  (`ThreadedCompositor.cpp:412, 506`), so steps() cannot help on GPU paths.
- **XWayland** (`GDK_BACKEND=x11`, x11 and x11-b): app 6.1-7.0% (no shm faults), but web stays
  at 37-50% (CPU rasterisation) and Xwayland takes 8.6-10.2%. Against base-d with the same
  animation set: client tree 53.0 vs 80.6, kwin 14.4 vs 32.1, plus Xwayland 10.2. Better than the
  default, far worse than DMA-BUF.
- **Stepped pulses** on the shared-memory path: there the animations run on the main thread and
  WebKit ticks only at step boundaries (`KeyframeEffect::timeToNextTick`,
  `progressUntilNextStep`), and frames are produced only when a value changes. Clean point (no
  Zerus mark, 8 stepped pulses, steps(4) via `@property`): 6.1 paints/s, app 3.3%, web 2.7%
  (prop after_hide). steps(4) and steps(12) on the original properties ran only with the mark
  spinning, which alone costs about 37 paints/s and app 14-17%; they added about 2 and 6 paints/s
  on top (steps, steps12). With the staggered typing dots also stepped, steps(12) saturated at
  60 paints/s (all12). **Estimated**: steps(12) on everything in phase, without the mark, would
  give about 15 paints/s, app about 7%. Visual identity was not checked.
- **`will-change: opacity, scale`** on the indicators (layers): app 27.0%, unchanged; web 26.9%
  against 34.9% in base-c (noise between runs is of that order, so no claim).

Shim overhead and noise: the shim adds two intercepted calls per paint (negligible,
**inferred**); I could not separate it from the drift of the live page. The earlier session's
run without shim read 33.6% (count-pulse x5, halo x1); base-shim read 47.6% (count-pulse x4,
halo x2). Between runs with similar animation sets the spread is about 5-10 points of the total
and about 3 points of the app (base-c 76.7/31.7, base-d 80.6/28.9, layers 61.8/27.0).

## Client-side renderer policy (proposal)

Written because the explicit-sync run held up twice (on-nvsync 12:25, on-nvsync-b 13:24, both
5 min each, no crash, 60 GL paints/s, readback showed UI colours). It is designed so the owner's
answer about why `/etc/environment` sets the variable slots in at step 3.

1. **Mechanism.** At the top of `main()`, before Tauri, GTK, WebKit or any thread starts:
   `unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "0"); std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1"); }`
   (`set_var` is unsafe in Rust 2024; it is sound only while the process is single-threaded).
   Verified effect: "0" re-enables the DMA-BUF renderer (source, and on-nvsync used `unset`, which
   WebKit treats the same); the explicit-sync string is read by libnvidia-egl-wayland2 1.0.2,
   libnvidia-egl-wayland 1.1.24 and the 615.78.08 eglcore/glcore (strings checked). WebKit's child
   processes inherit the environment. For shared memory set or keep `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
2. **Detection** (apply only for the NVIDIA proprietary driver on Wayland):
   - NVIDIA: `/proc/driver/nvidia/version` exists (proprietary userspace; present here, "NVIDIA
     UNIX Open Kernel Module 615.78.08") and the DRM card that drives the desktop uses the nvidia
     driver (`/sys/class/drm/card*/device/driver` -> `nvidia`, here card1 with `boot_vga=1`).
     `/sys/module/nvidia_drm/parameters/modeset` = `Y` here and is required for Wayland, so check it
     too. Not verified: hybrid laptops (nvidia loaded but the desktop on Intel/AMD), nouveau/NVK,
     several NVIDIA cards. The exact test is the EGL vendor of GTK's Wayland display, which is only
     known after GTK initialises, too late for the variables (**inferred**).
   - Wayland: decide as GDK does, not by `XDG_SESSION_TYPE`. In this session's agent shells
     `XDG_SESSION_TYPE=tty` while the desktop is Wayland and GTK connects to `wayland-0` even with
     `WAYLAND_DISPLAY` unset. Rule: `GDK_BACKEND` unset or starting with `wayland`, and a socket at
     `$WAYLAND_DISPLAY` or `$XDG_RUNTIME_DIR/wayland-0`. With `GDK_BACKEND=x11` leave everything as
     is (the DMA-BUF path on X11 uses a separate GL display and CPU mapping, `AcceleratedBackingStore.cpp:728-737`; not tested).
3. **Precedence.** The client cannot tell a global `WEBKIT_DISABLE_DMABUF_RENDERER=1` from one set
   for it, and a wrong override can show an empty window, which no crash marker catches.
   Recommendation: **option (a), refined.** `ZERUS_CLIENT_RENDERER=auto|dmabuf|shm` (and a client
   setting that takes effect after a restart), default `auto`:
   - `auto`, variable absent (stock WebKit would pick DMA-BUF and, on this stack, die with
     Error 71): on NVIDIA Wayland add `__NV_DISABLE_EXPLICIT_SYNC=1`; elsewhere change nothing.
   - `auto`, variable set to anything but "0": respect it, keep shared memory, and say so once in
     the diagnostics/settings ("renderer: shared memory, set by the system").
   - `dmabuf`: `WEBKIT_DISABLE_DMABUF_RENDERER=0`, plus `__NV_DISABLE_EXPLICIT_SYNC=1` on NVIDIA
     Wayland, guarded by the crash marker below.
   - `shm`: `WEBKIT_DISABLE_DMABUF_RENDERER=1` always.
   Option (b), overriding with blank-output detection, is possible but not ready: I verified only
   that a `glReadPixels` of GTK's back buffer right after `gdk_cairo_draw_from_gl` returns the UI's
   colours on this machine (first frame transparent, second white, from the tenth `#161b21`,
   `#1c2229`, `#242d36`, `#272e37` at five points, `glGetError` 0). I had no failing case, so I could
   not show it catches a black or garbled window; it sees GTK's back buffer, not KWin's output; and
   WebKit's snapshot API renders in the web process, so it would not see a UI-side import failure
   (**inferred**). If the owner's answer is "only for Error 71 crashes", the coordinator may let
   `auto` override with the marker; if it is "blank or garbled windows" (tauri-apps/tauri#9394,
   bugs.webkit.org 261874), keep (a).
4. **Failure modes and fallback.**
   - Error 71 is a hard exit within about 1 s, before the page logs anything (6/6 runs). Write a
     marker (renderer, driver version, time) before GTK starts; clear it when the page reports its
     first `requestAnimationFrame` after load. A marker found at start means the last DMA-BUF start
     did not reach a frame: use shared memory for this start and log why. **Inferred** sufficient
     for Error 71 (it hit before any page output in every crash); not for blank output.
   - The explicit-sync variable stops working or is no longer needed after a driver or
     egl-wayland update (two EGL platform libraries read it today; egl-wayland2 takes precedence via
     `09_nvidia_wayland2.json`): the marker turns this into one failed start. Key the marker to the
     driver version so an update retries DMA-BUF once (**inferred**).
   - Minimised windows keep painting on the GL path (60 paints/s, 18.5-21.9% while KWin reports
     minimised) where the shared-memory path drops to about 6%. A Wayland client cannot learn it was
     minimised, so this needs WebKit's own visibility, or pausing motion on focus loss (**inferred**).
   - Not observed but possible with implicit sync on NVIDIA: tearing or a stale frame (**inferred**;
     nothing was inspected visually).
   - XWayland (`GDK_BACKEND=x11`): untested with DMA-BUF; keep shared memory there.
5. **Cost and evidence** (sha256 prefixes of `runs/<run>/client.log`, `samples.jsonl`, `report.json`):

   | run | window | total | app | web | kwin | client.log | samples | report |
   |---|---|---|---|---|---|---|---|---|
   | base-c (SHM) | as displayed | 76.7 | 31.7 | 34.9 | 32.3 | def82ed29576 | 047f84677c1a | 5111adff8f96 |
   | base-d (SHM) | as displayed | 80.6 | 28.9 | 41.8 | 32.1 | 6b72f0426370 | 8558fe472d01 | f60cfa8d41d2 |
   | on-nvsync (DMA-BUF) | as displayed | 17.7 | 5.5 | 5.5 | 9.9 | 34308165f818 | 4a7b1355d3cf | 59722a913573 |
   | on-nvsync-b (DMA-BUF) | as displayed | 20.5 | 6.5 | 4.8 | 7.2 | 1d68bbd581ea | 969dfbe27c0e | 4c0370da022b |
   | base-shim (SHM, pre-restart) | as displayed | 47.6 | 26.9 | 13.3 | n/a | 3ae301e13693 | f7dbc2b1b939 | e2644520715b |
   | dmabuf-on (DMA-BUF alone) | died at start | | | | | a8e1ec1eb4d2 | | |

   Reduce motion is about 8-11% total on both paths; hidden about 9% on both.

## Commands run

All tooling is in the scratchpad folder `spike29/`; nothing was installed system-wide and no
system configuration was changed.

- Throwaway builds: `git -C <main worktree> worktree add --detach <spike worktree> 0a6fa30b`
  (the commit of the measured binary; origin/main had moved to 1f388605 with changes outside the
  frontend), `node_modules` symlinked, the p2 cargo target copied into the worktree as a warm cache,
  then per variant only `client/src/ui/styles/motion.css` edited and
  `RUSTUP_TOOLCHAIN=1.99.1 CARGO_BUILD_JOBS=6 nice npx tauri build --no-bundle`, the binary copied
  to `src-tauri/target-v/<variant>/`. Removed with `git worktree remove --force` at the end.
  Variants: `steps` (`.gb-count.working, .sbadge.k-working .sb-mark::after { animation-timing-function: steps(4); }`),
  `prop` (the same steps, the pulse and halo driven by `@property --spike-pulse/--spike-halo`),
  `steps12` (steps(12)), `all4`/`all12` (steps(4)/(12) also on `.status.working .dot, .state-dot.working, .typing i`),
  `layers` (`will-change: opacity, scale` on those selectors, smooth).
- Each display run, one at a time: `python3 client/bench/display-info.py --require-visible`, then
  `flock -w 1800 /tmp/zerus-arch-display.lock python3 runv.py --name <run> --bin <binary> [--shim] [--profile] [--backend x11] [--env K=V] [--unset K]`.
  `runv.py` starts the client with `ZERUS_CLIENT_PERF=auto` (`env -u DISPLAY GDK_BACKEND=wayland`,
  or `GDK_BACKEND=x11` with DISPLAY), stamps its stderr, samples `/proc` once a second outside the
  measured tree (per-thread CPU, user time, minor faults, context switches; kwin_wayland and
  Xwayland CPU), loads a read-only KWin script that reports only the test window's minimized state,
  and stops and discards the run if the session locks or a screen sleeps (that happened 5 times;
  each run was repeated). `--shim` preloads `shim/paintcount.so`, which counts
  `gdk_window_begin_draw_frame` and `gdk_cairo_draw_from_gl` calls (and, with `SPIKE_READBACK=1`,
  reads five pixels back after the GL draw).
- Profile: perf 7.2.10 from the Arch package (sha256 checked against the signed sync database,
  unpacked into the scratchpad, not installed), `perf_event_paranoid` 2:
  `perf record -N -m 64 -e cycles:u -F 199 --call-graph dwarf,16384 -p <app>,<WebKitWebProcess> -- sleep 9`,
  `perf report --stdio --children|--no-children --pid <pid> --sort dso|sym`; debug info for GTK,
  GDK, cairo, pixman, GLib, GObject, wayland-client, libc, epoxy, EGL from debuginfod.archlinux.org.
- Source reading: webkitgtk-2.54.1 and gtk-3.24.52 release tarballs (sha256 of gtk matched
  download.gnome.org).
- Analysis: `analyze.py`, `summary.py`, `settle.py`, `perf-report.sh` in `spike29/`; raw data in
  `spike29/runs/<run>/`, perf reports in `spike29/prof/`.

## Unexplained

- Why the web process costs 13% with six pulses but 35-61% once the Activity's typing dots and
  status dots also pulse (base-shim against base-a, base-c, base-d). Without WebKit symbols I could
  not see which layers re-rasterise.
- Why GTK briefly paints 96-123 times a second on the shared-memory path during the settle after
  the scenarios (2.6-5.6 s after "scenarios done").
- Why the GL path paints at 60 fps rather than at the 144 Hz frame callbacks, while the
  FORCE_SHM path painted at 136 fps.
- Kimi's DMA-BUF client at 3.2% app at 144 Hz is cheaper per frame than my stock GL path.
