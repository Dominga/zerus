/* Exercise the real GTK3 clock through its backend vtable. Run only through
 * test-gtk-frame-clock.py, which creates and retains a private Xvfb process.
 * The uninstalled header comes from the exact GTK source being tested. */
// Keep the public umbrella before GTK's uninstalled header.
// clang-format off
#include <gdk/gdk.h>
#include "gdkframeclockprivate.h"
// clang-format on
#include <stdio.h>
#include <string.h>

typedef struct {
  GMainLoop *loop;
  GdkFrameClock *clock;
  guint paints, layouts, after_paints, thaw_source;
  gboolean frozen, expired;
} Probe;

static gboolean thaw_clock(gpointer data) {
  Probe *p = data;
  p->thaw_source = 0;
  p->frozen = FALSE;
  ((GdkFrameClockClass *)G_OBJECT_GET_CLASS(p->clock))->thaw(p->clock);
  return G_SOURCE_REMOVE;
}
static void layout(GdkFrameClock *clock, Probe *p) { p->layouts++; }
static void after_paint(GdkFrameClock *clock, Probe *p) { p->after_paints++; }
static void paint(GdkFrameClock *clock, Probe *p) {
  if (++p->paints == 32) {
    g_main_loop_quit(p->loop);
    return;
  }
  /* The backend's compositor callback arrives after this paint returns.
   * A continuous producer requests a fresh layout and paint before then. */
  ((GdkFrameClockClass *)G_OBJECT_GET_CLASS(clock))->freeze(clock);
  p->frozen = TRUE;
  gdk_frame_clock_request_phase(clock, GDK_FRAME_CLOCK_PHASE_LAYOUT |
                                           GDK_FRAME_CLOCK_PHASE_PAINT);
  p->thaw_source = g_timeout_add(1, thaw_clock, p);
}
static gboolean deadline(gpointer data) {
  Probe *p = data;
  p->expired = TRUE;
  g_main_loop_quit(p->loop);
  return G_SOURCE_REMOVE;
}
int main(int argc, char **argv) {
  const char *marker = g_getenv("ZERUS_GTK_FRAME_CLOCK_TEST");
  const char *parent = g_getenv("ZERUS_GTK_PARENT_DISPLAY");
  const char *display = g_getenv("DISPLAY");
  if (argc != 3 || strcmp(argv[1], "--exact") ||
      strcmp(argv[2], "paint_freeze_resumes_after_paint") || !marker ||
      !*marker || !display || !parent || !strcmp(display, parent) ||
      g_strcmp0(g_getenv("GDK_BACKEND"), "x11"))
    return 2;
  if (!gdk_init_check(NULL, NULL))
    return 2;
  GdkWindowAttr attributes = {0};
  attributes.window_type = GDK_WINDOW_TOPLEVEL;
  attributes.wclass = GDK_INPUT_OUTPUT;
  attributes.width = attributes.height = 1;
  GdkWindow *window = gdk_window_new(NULL, &attributes, 0);
  GdkFrameClock *native = gdk_window_get_frame_clock(window);
  Probe p = {0};
  p.loop = g_main_loop_new(NULL, FALSE);
  /* A detached instance of the real backend clock, without a window's own
   * paint callbacks, isolates the scheduling contract deterministically. */
  p.clock = g_object_new(G_OBJECT_TYPE(native), NULL);
  g_signal_connect(p.clock, "layout", G_CALLBACK(layout), &p);
  g_signal_connect(p.clock, "paint", G_CALLBACK(paint), &p);
  g_signal_connect(p.clock, "after-paint", G_CALLBACK(after_paint), &p);
  guint timer = g_timeout_add_seconds(3, deadline, &p);
  gdk_frame_clock_request_phase(p.clock, GDK_FRAME_CLOCK_PHASE_LAYOUT |
                                             GDK_FRAME_CLOCK_PHASE_PAINT);
  g_main_loop_run(p.loop);
  if (!p.expired)
    g_source_remove(timer);
  if (p.thaw_source) {
    g_source_remove(p.thaw_source);
    thaw_clock(&p);
  }
  gboolean passed =
      !p.expired && p.paints == 32 && p.layouts == 32 && p.after_paints == 32;
  printf("{\"paints\":%u,\"layouts\":%u,\"after_paints\":%u,\"expired\":%s,"
         "\"passed\":%s}\n",
         p.paints, p.layouts, p.after_paints, p.expired ? "true" : "false",
         passed ? "true" : "false");
  g_object_unref(p.clock);
  g_main_loop_unref(p.loop);
  gdk_window_destroy(window);
  return passed ? 0 : 1;
}
