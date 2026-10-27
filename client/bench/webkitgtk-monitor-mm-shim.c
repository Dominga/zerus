// LD_PRELOAD experiment for zerus-5wmh.14: replaces the physical size that GDK
// reports for a monitor, so WebKitGTK's DRM vblank lookup can find the connector.
//
// WebKitGTK 2.54 (Source/WebKit/UIProcess/glib/DisplayVBlankMonitorDRM.cpp, findCrtc)
// only accepts a DRM connector whose mmWidth and mmHeight equal
// gdk_monitor_get_width_mm() and gdk_monitor_get_height_mm(). KWin reports sizes
// to clients that differ by a few millimetres from the kernel connector's sizes,
// so the lookup fails and WebKit paces with its 60 Hz timer. This shim maps the
// size GDK reports to the connector's size; nothing else changes.
//
//   ZERUS_GDK_MONITOR_MM="597x336=600x340,797x334=800x330"   (GDK size = DRM size)
//   ZERUS_GDK_MONITOR_MM="*=600x340"                          (any monitor)
//
// Build (no GTK headers needed; the real functions are found with dlsym):
//   cc -O2 -shared -fPIC -o client/bench/build/webkitgtk-monitor-mm-shim.so client/bench/webkitgtk-monitor-mm-shim.c -ldl
// Used by client/bench/webkitgtk-vblank-experiments.sh; never part of the client.

#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef int (*MonitorMmFunction)(void*);

static int mapped(void* monitor, int wantHeight)
{
    static MonitorMmFunction realWidth;
    static MonitorMmFunction realHeight;
    static int logged;
    if (!realWidth)
        realWidth = (MonitorMmFunction)dlsym(RTLD_NEXT, "gdk_monitor_get_width_mm");
    if (!realHeight)
        realHeight = (MonitorMmFunction)dlsym(RTLD_NEXT, "gdk_monitor_get_height_mm");
    if (!realWidth || !realHeight)
        return 0;

    int width = realWidth(monitor);
    int height = realHeight(monitor);
    const char* map = getenv("ZERUS_GDK_MONITOR_MM");
    if (!map)
        return wantHeight ? height : width;

    // Entries "WxH=WxH" or "*=WxH", separated by commas.
    const char* entry = map;
    while (entry && *entry) {
        int fromWidth = -1, fromHeight = -1, toWidth = 0, toHeight = 0;
        int matches = 0;
        if (entry[0] == '*')
            matches = sscanf(entry, "*=%dx%d", &toWidth, &toHeight) == 2;
        else if (sscanf(entry, "%dx%d=%dx%d", &fromWidth, &fromHeight, &toWidth, &toHeight) == 4)
            matches = fromWidth == width && fromHeight == height;
        if (matches) {
            if (logged < 8) {
                logged++;
                fprintf(stderr, "[zerus-mm-shim] GDK monitor %dx%d mm reported as %dx%d mm\n", width, height, toWidth, toHeight);
            }
            return wantHeight ? toHeight : toWidth;
        }
        entry = strchr(entry, ',');
        if (entry)
            entry++;
    }
    return wantHeight ? height : width;
}

int gdk_monitor_get_width_mm(void* monitor)
{
    return mapped(monitor, 0);
}

int gdk_monitor_get_height_mm(void* monitor)
{
    return mapped(monitor, 1);
}
