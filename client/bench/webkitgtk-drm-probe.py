#!/usr/bin/env python3
"""Window-less check of the DRM vblank path WebKitGTK uses to pace rendering on Linux.

WebKitGTK 2.54 (Source/WebKit/UIProcess/glib/DisplayVBlankMonitorDRM.cpp) picks
the DRM connector whose physical size in millimetres equals the GdkMonitor's
(gdk_monitor_get_width_mm/height_mm, from wl_output.geometry on Wayland), then
waits for vblanks on the CRTC that drives it. If no connector matches, it falls
back to a 60 Hz timer (DisplayVBlankMonitorTimer.cpp sleeps 1000 / 60 = 16 ms).

This script repeats that lookup with libdrm, as WebKit does it (open the primary
node read-write as an unprivileged client, no master, no mode probe), prints each
connected connector with its size and CRTC (legacy fields, which WebKit reads, and
the atomic CRTC_ID), and times a few drmWaitVBlank calls on every CRTC. With
--compositor it also reads the compositor's output sizes (`kscreen-doctor --json`,
KDE) and says whether WebKit's connector lookup would succeed. It opens no window
and changes no state. Only connector names, sizes, modes and CRTC indices are
printed; never EDID, model or serial.

    python3 client/bench/webkitgtk-drm-probe.py [--compositor] [--json]
"""
import argparse
import ctypes
import ctypes.util
import glob
import json
import os
import subprocess
import sys
import time

DRM_MODE_CONNECTED = 1
DRM_VBLANK_RELATIVE = 0x1
DRM_VBLANK_SECONDARY = 0x20000000
DRM_VBLANK_HIGH_CRTC_SHIFT = 1
DRM_VBLANK_HIGH_CRTC_MASK = 0x3E
CONNECTOR_TYPES = {
    1: "VGA", 2: "DVI-I", 3: "DVI-D", 4: "DVI-A", 5: "Composite", 6: "SVIDEO", 7: "LVDS", 8: "Component",
    9: "DIN", 10: "DP", 11: "HDMI-A", 12: "HDMI-B", 13: "TV", 14: "eDP", 15: "Virtual", 16: "DSI", 17: "DPI",
    18: "Writeback", 19: "SPI", 20: "USB",
}


class ModeInfo(ctypes.Structure):
    _fields_ = [
        ("clock", ctypes.c_uint32),
        ("hdisplay", ctypes.c_uint16), ("hsync_start", ctypes.c_uint16), ("hsync_end", ctypes.c_uint16),
        ("htotal", ctypes.c_uint16), ("hskew", ctypes.c_uint16),
        ("vdisplay", ctypes.c_uint16), ("vsync_start", ctypes.c_uint16), ("vsync_end", ctypes.c_uint16),
        ("vtotal", ctypes.c_uint16), ("vscan", ctypes.c_uint16),
        ("vrefresh", ctypes.c_uint32), ("flags", ctypes.c_uint32), ("type", ctypes.c_uint32),
        ("name", ctypes.c_char * 32),
    ]


class Res(ctypes.Structure):
    _fields_ = [
        ("count_fbs", ctypes.c_int), ("fbs", ctypes.POINTER(ctypes.c_uint32)),
        ("count_crtcs", ctypes.c_int), ("crtcs", ctypes.POINTER(ctypes.c_uint32)),
        ("count_connectors", ctypes.c_int), ("connectors", ctypes.POINTER(ctypes.c_uint32)),
        ("count_encoders", ctypes.c_int), ("encoders", ctypes.POINTER(ctypes.c_uint32)),
        ("min_width", ctypes.c_uint32), ("max_width", ctypes.c_uint32),
        ("min_height", ctypes.c_uint32), ("max_height", ctypes.c_uint32),
    ]


class Connector(ctypes.Structure):
    _fields_ = [
        ("connector_id", ctypes.c_uint32), ("encoder_id", ctypes.c_uint32),
        ("connector_type", ctypes.c_uint32), ("connector_type_id", ctypes.c_uint32),
        ("connection", ctypes.c_int), ("mmWidth", ctypes.c_uint32), ("mmHeight", ctypes.c_uint32),
        ("subpixel", ctypes.c_int), ("count_modes", ctypes.c_int), ("modes", ctypes.POINTER(ModeInfo)),
        ("count_props", ctypes.c_int), ("props", ctypes.POINTER(ctypes.c_uint32)),
        ("prop_values", ctypes.POINTER(ctypes.c_uint64)),
        ("count_encoders", ctypes.c_int), ("encoders", ctypes.POINTER(ctypes.c_uint32)),
    ]


class Encoder(ctypes.Structure):
    _fields_ = [
        ("encoder_id", ctypes.c_uint32), ("encoder_type", ctypes.c_uint32), ("crtc_id", ctypes.c_uint32),
        ("possible_crtcs", ctypes.c_uint32), ("possible_clones", ctypes.c_uint32),
    ]


class Crtc(ctypes.Structure):
    _fields_ = [
        ("crtc_id", ctypes.c_uint32), ("buffer_id", ctypes.c_uint32), ("x", ctypes.c_uint32), ("y", ctypes.c_uint32),
        ("width", ctypes.c_uint32), ("height", ctypes.c_uint32), ("mode_valid", ctypes.c_int),
        ("mode", ModeInfo), ("gamma_size", ctypes.c_int),
    ]


class ObjectProperties(ctypes.Structure):
    _fields_ = [("count_props", ctypes.c_uint32), ("props", ctypes.POINTER(ctypes.c_uint32)),
                ("prop_values", ctypes.POINTER(ctypes.c_uint64))]


class Property(ctypes.Structure):
    _fields_ = [("prop_id", ctypes.c_uint32), ("flags", ctypes.c_uint32), ("name", ctypes.c_char * 32)]


class VBlankReq(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("sequence", ctypes.c_uint), ("signal", ctypes.c_ulong)]


class VBlankReply(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("sequence", ctypes.c_uint), ("tval_sec", ctypes.c_long), ("tval_usec", ctypes.c_long)]


class VBlank(ctypes.Union):
    _fields_ = [("request", VBlankReq), ("reply", VBlankReply)]


def load():
    drm = ctypes.CDLL(ctypes.util.find_library("drm") or "libdrm.so.2", use_errno=True)
    drm.drmModeGetResources.restype = ctypes.POINTER(Res)
    drm.drmModeGetConnector.restype = ctypes.POINTER(Connector)
    drm.drmModeGetConnector.argtypes = [ctypes.c_int, ctypes.c_uint32]
    drm.drmModeGetEncoder.restype = ctypes.POINTER(Encoder)
    drm.drmModeGetEncoder.argtypes = [ctypes.c_int, ctypes.c_uint32]
    drm.drmModeGetCrtc.restype = ctypes.POINTER(Crtc)
    drm.drmModeGetCrtc.argtypes = [ctypes.c_int, ctypes.c_uint32]
    drm.drmWaitVBlank.argtypes = [ctypes.c_int, ctypes.POINTER(VBlank)]
    drm.drmGetDevices2.argtypes = [ctypes.c_uint32, ctypes.c_void_p, ctypes.c_int]
    drm.drmSetClientCap.argtypes = [ctypes.c_int, ctypes.c_uint64, ctypes.c_uint64]
    drm.drmModeObjectGetProperties.restype = ctypes.POINTER(ObjectProperties)
    drm.drmModeObjectGetProperties.argtypes = [ctypes.c_int, ctypes.c_uint32, ctypes.c_uint32]
    drm.drmModeGetProperty.restype = ctypes.POINTER(Property)
    drm.drmModeGetProperty.argtypes = [ctypes.c_int, ctypes.c_uint32]
    return drm


DRM_CLIENT_CAP_ATOMIC = 3
DRM_MODE_OBJECT_CONNECTOR = 0xC0C0C0C0
DRM_MODE_OBJECT_CRTC = 0xCCCCCCCC


def object_props(drm, fd, object_id, object_type, names):
    """Values of the named properties of a DRM object (atomic state)."""
    values = {}
    props_ptr = drm.drmModeObjectGetProperties(fd, object_id, object_type)
    if not props_ptr:
        return values
    props = props_ptr.contents
    for i in range(props.count_props):
        prop_ptr = drm.drmModeGetProperty(fd, props.props[i])
        if not prop_ptr:
            continue
        name = prop_ptr.contents.name.decode(errors="replace")
        drm.drmModeFreeProperty(prop_ptr)
        if name in names:
            values[name] = props.prop_values[i]
    drm.drmModeFreeObjectProperties(props_ptr)
    return values


def mode_hz(mode):
    """The refresh rate of a mode in Hz (the formula of WebKit's WPE findCrtc)."""
    if not mode.htotal or not mode.vtotal:
        return None
    hz = mode.clock * 1000.0 / mode.htotal / mode.vtotal
    if mode.flags & 0x10:  # DRM_MODE_FLAG_INTERLACE
        hz *= 2
    if mode.flags & 0x20:  # DRM_MODE_FLAG_DBLSCAN
        hz /= 2
    if mode.vscan > 1:
        hz /= mode.vscan
    return round(hz, 3)


def crtc_bitmask(index):
    """DisplayVBlankMonitorDRM.cpp crtcBitmaskForIndex()."""
    if index > 1:
        return (index << DRM_VBLANK_HIGH_CRTC_SHIFT) & DRM_VBLANK_HIGH_CRTC_MASK
    return DRM_VBLANK_SECONDARY if index > 0 else 0


def wait_vblanks(drm, fd, index, count):
    intervals, errors, last = [], [], None
    for _ in range(count + 1):
        vblank = VBlank()
        vblank.request.type = DRM_VBLANK_RELATIVE | crtc_bitmask(index)
        vblank.request.sequence = 1
        ret = drm.drmWaitVBlank(fd, ctypes.byref(vblank))
        now = time.perf_counter()
        if ret:
            errors.append(os.strerror(ctypes.get_errno() or -ret))
            break
        if last is not None:
            intervals.append((now - last) * 1000)
        last = now
    mean = round(sum(intervals) / len(intervals), 3) if intervals else None
    return {"waits": len(intervals), "mean_interval_ms": mean, "error": errors[0] if errors else None}


def probe_node(drm, node, vblanks):
    info = {"node": node, "connectors": [], "error": None}
    try:
        fd = os.open(node, os.O_RDWR | os.O_CLOEXEC)  # what WebKit does (DisplayVBlankMonitorDRM.cpp)
    except OSError as error:
        info["error"] = f"open: {error.strerror}"
        return info
    try:
        res_ptr = drm.drmModeGetResources(fd)
        if not res_ptr:
            info["error"] = "drmModeGetResources failed"
            return info
        res = res_ptr.contents
        crtcs = [res.crtcs[i] for i in range(res.count_crtcs)]
        info["crtc_count"] = len(crtcs)
        connected_ids = []
        for i in range(res.count_connectors):
            conn_ptr = drm.drmModeGetConnector(fd, res.connectors[i])
            if not conn_ptr:
                continue
            conn = conn_ptr.contents
            name = f"{CONNECTOR_TYPES.get(conn.connector_type, conn.connector_type)}-{conn.connector_type_id}"
            entry = {
                "name": name, "connected": conn.connection == DRM_MODE_CONNECTED,
                "encoder_id": conn.encoder_id, "modes": conn.count_modes,
                "size_mm": [conn.mmWidth, conn.mmHeight],
            }
            drm.drmModeFreeConnector(conn_ptr)
            if not entry["connected"]:
                continue
            connected_ids.append(res.connectors[i])
            # WebKit skips a connector without encoder_id or modes (findCrtc).
            entry["webkit_candidate"] = bool(entry["encoder_id"] and entry["modes"])
            if entry["encoder_id"]:
                enc_ptr = drm.drmModeGetEncoder(fd, entry["encoder_id"])
                if enc_ptr:
                    crtc_id = enc_ptr.contents.crtc_id
                    drm.drmModeFreeEncoder(enc_ptr)
                    if crtc_id in crtcs:
                        entry["crtc_index"] = crtcs.index(crtc_id)
                        crtc_ptr = drm.drmModeGetCrtc(fd, crtc_id)
                        if crtc_ptr:
                            crtc = crtc_ptr.contents
                            entry["crtc_mode"] = f"{crtc.mode.hdisplay}x{crtc.mode.vdisplay}" if crtc.mode_valid else None
                            entry["crtc_hz"] = mode_hz(crtc.mode) if crtc.mode_valid else None
                            drm.drmModeFreeCrtc(crtc_ptr)
            info["connectors"].append(entry)
        # The atomic state (what KWin programs; WebKit reads only the legacy fields above).
        if drm.drmSetClientCap(fd, DRM_CLIENT_CAP_ATOMIC, 1) == 0:
            for entry, connector_id in zip(info["connectors"], connected_ids):
                crtc_id = object_props(drm, fd, connector_id, DRM_MODE_OBJECT_CONNECTOR, {"CRTC_ID"}).get("CRTC_ID")
                atomic = {"crtc_id": crtc_id}
                if crtc_id in crtcs:
                    atomic["crtc_index"] = crtcs.index(crtc_id)
                    atomic["active"] = object_props(drm, fd, crtc_id, DRM_MODE_OBJECT_CRTC, {"ACTIVE"}).get("ACTIVE")
                entry["atomic"] = atomic
        else:
            info["atomic_error"] = "DRM_CLIENT_CAP_ATOMIC refused"
        # The call WebKit's vblank thread makes, on every CRTC: EOPNOTSUPP means the driver has no
        # DRM vblank support at all (nvidia-drm without vblank=1), EINVAL an inactive CRTC.
        if vblanks:
            info["crtc_vblank"] = [dict(index=i, **wait_vblanks(drm, fd, i, vblanks)) for i in range(len(crtcs))]
        drm.drmModeFreeResources(res_ptr)
    finally:
        os.close(fd)
    return info


def compositor_outputs():
    """Enabled outputs of KWin: name, mode, refresh and the physical size it reports to clients."""
    try:
        raw = subprocess.run(["kscreen-doctor", "--json"], capture_output=True, text=True, timeout=20, check=True).stdout
    except (OSError, subprocess.SubprocessError) as error:
        return {"error": str(error)}
    outputs = []
    for output in json.loads(raw).get("outputs", []):
        if not output.get("enabled"):
            continue
        mode = next((m for m in output.get("modes", []) if m.get("id") == output.get("currentModeId")), {})
        size = mode.get("size") or {}
        mm = output.get("sizeMM") or {}
        outputs.append({
            "name": output.get("name"), "mode": f"{size.get('width')}x{size.get('height')}",
            "refresh_hz": mode.get("refreshRate"), "size_mm": [mm.get("width"), mm.get("height")],
            "rotation": output.get("rotation"),
        })
    return {"source": "kscreen-doctor --json", "outputs": outputs}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--compositor", action="store_true", help="compare with the compositor's output sizes (KDE)")
    parser.add_argument("--vblanks", type=int, default=20, help="drmWaitVBlank calls per active CRTC (0: none)")
    parser.add_argument("--json", action="store_true", help="print JSON instead of text")
    args = parser.parse_args()
    if not sys.platform.startswith("linux"):
        sys.exit("Linux only")

    drm = load()
    report = {"drm_devices": drm.drmGetDevices2(0, None, 0), "nodes": []}
    for node in sorted(glob.glob("/dev/dri/card*")):
        report["nodes"].append(probe_node(drm, node, args.vblanks))
    if args.compositor:
        report["compositor"] = compositor_outputs()
        connectors = [c for n in report["nodes"] for c in n["connectors"]]
        for output in report["compositor"].get("outputs", []):
            # findCrtc: connected, encoder_id and modes, and the same size in mm as the GdkMonitor.
            same_size = [c for c in connectors if c["size_mm"] == output["size_mm"]]
            usable = [c for c in same_size if c.get("webkit_candidate")]
            output["webkit_size_match"] = [c["name"] for c in usable]
            if not same_size:
                kernel = next((c["size_mm"] for c in connectors if c["name"] == output["name"]), None)
                reason = f"no connector of the same size (kernel {output['name']}: {kernel[0]}x{kernel[1]} mm)" if kernel \
                    else "no connector of the same size"
            elif not usable:
                reason = "the connector of that size has no encoder (output off, or routing not visible)"
            else:
                reason = None
            output["webkit_lookup"] = "matches " + usable[0]["name"] if usable else "fails: " + reason
    if args.json:
        print(json.dumps(report, indent=2))
        return
    print(f"libdrm devices: {report['drm_devices']}")
    for node in report["nodes"]:
        print(f"{node['node']}: {node['error'] or str(node.get('crtc_count')) + ' CRTCs'}")
        for c in node["connectors"]:
            route = (f"CRTC index {c['crtc_index']}, {c.get('crtc_mode')} at {c.get('crtc_hz')} Hz"
                     if c.get("crtc_index") is not None else "no CRTC through the encoder")
            print(f"  {c['name']}: {c['size_mm'][0]}x{c['size_mm'][1]} mm, encoder_id {c['encoder_id']}, {c['modes']} modes, {route}")
            if "atomic" in c:
                a = c["atomic"]
                print(f"    atomic CRTC_ID {a.get('crtc_id')}" + (f" (index {a['crtc_index']}, ACTIVE {a.get('active')})"
                                                                 if a.get("crtc_index") is not None else ""))
        for v in node.get("crtc_vblank", []):
            print(f"  drmWaitVBlank on CRTC index {v['index']}: "
                  + (f"error {v['error']}" if v.get("error") else f"mean {v['mean_interval_ms']} ms over {v['waits']} waits"))
    if args.compositor:
        comp = report["compositor"]
        if comp.get("error"):
            print(f"compositor: {comp['error']}")
        for o in comp.get("outputs", []):
            print(f"compositor {o['name']}: {o['mode']} at {o['refresh_hz']} Hz, reports {o['size_mm'][0]}x{o['size_mm'][1]} mm"
                  f" -> WebKit DRM connector lookup {o['webkit_lookup']}")


if __name__ == "__main__":
    main()
