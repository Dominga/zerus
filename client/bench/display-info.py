#!/usr/bin/env python3
"""Display modes for the results, and attaching them to a result file.

    python3 client/bench/display-info.py                  # prints the displays as JSON
    python3 client/bench/display-info.py --attach FILE    # adds them as platform.displays

Linux (KDE): `kscreen-doctor --json`. Only the output name, mode size, refresh rate,
scale, priority, rotation and VRR policy are kept; never EDID, model or serial.
`--require-visible` exits with 3 when the graphical session is locked or a display
is in DPMS standby: a compositor then suspends new windows and they paint nothing.
"""
import argparse
import getpass
import json
import subprocess
import sys


def kscreen():
    raw = subprocess.run(["kscreen-doctor", "--json"], capture_output=True, text=True, timeout=20, check=True).stdout
    displays = []
    for output in json.loads(raw).get("outputs", []):
        if not output.get("enabled"):
            continue
        mode = next((m for m in output.get("modes", []) if m.get("id") == output.get("currentModeId")), {})
        size = mode.get("size") or {}
        displays.append({
            "name": output.get("name"),
            "priority": output.get("priority"),
            "mode": f"{size.get('width')}x{size.get('height')}",
            "refresh_hz": mode.get("refreshRate"),
            "scale": output.get("scale"),
            "rotation": output.get("rotation"),
            "vrr_policy": output.get("vrrPolicy"),
        })
    displays.sort(key=lambda d: d.get("priority") or 99)
    return {"source": "kscreen-doctor --json", "outputs": displays, "locked": locked(), "dpms": dpms()}


def locked():
    """LockedHint of the user's graphical session (loginctl), None when unknown."""
    try:
        user = getpass.getuser()
        session = subprocess.run(["loginctl", "show-user", user, "-p", "Display", "--value"],
                                 capture_output=True, text=True, timeout=10).stdout.strip()
        if not session:
            return None
        hint = subprocess.run(["loginctl", "show-session", session, "-p", "LockedHint", "--value"],
                              capture_output=True, text=True, timeout=10).stdout.strip()
        return {"yes": True, "no": False}.get(hint)
    except (OSError, subprocess.SubprocessError):
        return None


def dpms():
    """Output name -> DPMS mode ("on", "off", ...) from kscreen-doctor."""
    try:
        out = subprocess.run(["kscreen-doctor", "--dpms", "show"], capture_output=True, text=True, timeout=10).stdout
    except (OSError, subprocess.SubprocessError):
        return {}
    modes = {}
    for line in out.splitlines():
        # "dpms mode for screen DP-2: off"
        if line.startswith("dpms mode for screen ") and ":" in line:
            name, _, mode = line[len("dpms mode for screen "):].partition(":")
            modes[name.strip()] = mode.strip()
    return modes


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--attach", help="result file to add the displays to (platform.displays)")
    parser.add_argument("--require-visible", action="store_true", help="exit 3 when locked or a display sleeps")
    args = parser.parse_args()
    if not sys.platform.startswith("linux"):
        sys.exit("display modes are read with kscreen-doctor on Linux only")
    info = kscreen()
    if args.require_visible:
        asleep = [name for name, mode in info["dpms"].items() if mode != "on"]
        if info["locked"] or asleep:
            print(f"not visible: locked {info['locked']}, displays in standby {asleep or 'none'}", file=sys.stderr)
            sys.exit(3)
    if args.attach:
        with open(args.attach, encoding="utf-8") as handle:
            report = json.load(handle)
        report.setdefault("platform", {})["displays"] = info
        with open(args.attach, "w", encoding="utf-8") as handle:
            json.dump(report, handle, indent=2, ensure_ascii=False)
            handle.write("\n")
    else:
        print(json.dumps(info, indent=2))


if __name__ == "__main__":
    main()
