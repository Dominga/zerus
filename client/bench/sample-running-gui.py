#!/usr/bin/env python3
"""Read-only CPU and memory sample of a running GUI process tree (Linux and macOS).

Finds the process (default: the current user's `hgs-tray`), reads the CPU time of
it and its descendants at the start and the end of the window, and their memory at
the end. Linux reads /proc (RSS and PSS); macOS runs `ps` (RSS). It never signals,
attaches to or restarts anything. Output holds numbers and process names only,
never command lines or paths, which may contain session names.

    python3 client/bench/sample-running-gui.py [--name hgs-tray] [--pid N] [--seconds 60] [--out FILE]
"""
import argparse
import json
import os
import subprocess
import sys
import time

LINUX = sys.platform.startswith("linux")
CLOCK_TICKS = os.sysconf("SC_CLK_TCK") if LINUX else 100


def read(path):
    try:
        with open(path, "rb") as handle:
            return handle.read().decode("utf-8", "replace")
    except OSError:
        return None


def kb(path, key):
    text = read(path) or ""
    for line in text.splitlines():
        if line.startswith(key + ":"):
            return int(line.split()[1]) * 1024
    return None


def linux_processes():
    """pid -> {name, ppid, uid, cpu_seconds, reaped_seconds}."""
    result = {}
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        text = read(f"/proc/{entry}/stat")
        if not text:
            continue
        close = text.rfind(")")
        rest = text[close + 1:].split()
        # proc(5) field n is rest[n - 3]: ppid 4, utime 14, stime 15, cutime 16, cstime 17.
        status = read(f"/proc/{entry}/status") or ""
        uid = next((int(line.split()[1]) for line in status.splitlines() if line.startswith("Uid:")), None)
        result[int(entry)] = {
            "name": text[text.find("(") + 1:close],
            "ppid": int(rest[1]),
            "uid": uid,
            "cpu_seconds": (int(rest[11]) + int(rest[12])) / CLOCK_TICKS,
            "reaped_seconds": (int(rest[13]) + int(rest[14])) / CLOCK_TICKS,
        }
    return result


def cpu_time(text):
    """ps TIME "[[dd-]hh:]mm:ss[.cc]" to seconds."""
    days, _, rest = text.rpartition("-")
    seconds = 0.0
    for part in rest.split(":"):
        seconds = seconds * 60 + float(part)
    return seconds + (float(days) * 86400 if days else 0)


def mac_processes():
    out = subprocess.run(["/bin/ps", "-A", "-o", "pid=,ppid=,uid=,rss=,time=,comm="], capture_output=True, text=True, check=True).stdout
    result = {}
    for line in out.splitlines():
        fields = line.split(None, 5)
        if len(fields) < 6:
            continue
        pid, ppid, uid, rss, cputime, comm = fields
        result[int(pid)] = {
            "name": os.path.basename(comm.strip()),
            "ppid": int(ppid),
            "uid": int(uid),
            "cpu_seconds": cpu_time(cputime),
            "rss_bytes": int(rss) * 1024,
        }
    return result


def processes():
    return linux_processes() if LINUX else mac_processes()


def tree(root, table):
    members, queue = set(), [root]
    while queue:
        pid = queue.pop()
        if pid in members or pid not in table:
            continue
        members.add(pid)
        queue.extend(child for child, row in table.items() if row["ppid"] == pid and child != pid)
    return {pid: table[pid] for pid in members}


def memory(pid, row):
    if LINUX:
        return kb(f"/proc/{pid}/status", "VmRSS"), kb(f"/proc/{pid}/smaps_rollup", "Pss")
    return row.get("rss_bytes"), None


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--name", default="hgs-tray")
    parser.add_argument("--pid", type=int)
    parser.add_argument("--seconds", type=float, default=60.0)
    parser.add_argument("--label", default="")
    parser.add_argument("--out")
    args = parser.parse_args()

    table = processes()
    if args.pid:
        root = args.pid
    else:
        me = os.getuid()
        pids = [pid for pid, row in table.items() if row["name"] == args.name and row["uid"] == me]
        if len(pids) != 1:
            sys.exit(f"expected one {args.name} process of this user, found {len(pids)}")
        root = pids[0]

    first, started = tree(root, table), time.monotonic()
    reaped_before = first.get(root, {}).get("reaped_seconds")
    time.sleep(args.seconds)
    second, elapsed = tree(root, processes()), time.monotonic() - started
    if root not in second:
        sys.exit("the process exited during the sample")

    rows = []
    for pid, now in sorted(second.items()):
        before = first.get(pid)
        cpu = None
        if before and now["cpu_seconds"] >= before["cpu_seconds"]:
            cpu = round((now["cpu_seconds"] - before["cpu_seconds"]) / elapsed * 100, 3)
        rss, pss = memory(pid, now)
        rows.append({"role": "app" if pid == root else "child", "name": now["name"], "cpu_percent": cpu, "rss_bytes": rss, "pss_bytes": pss})
    app = next(row for row in rows if row["role"] == "app")
    gui = {
        "window_ms": round(elapsed * 1000),
        "source": "procfs" if LINUX else "ps",
        "app": {"cpu_percent": app["cpu_percent"], "rss_bytes": app["rss_bytes"], "pss_bytes": app["pss_bytes"]},
        "tree": {
            "processes": len(rows),
            "cpu_percent": round(sum(row["cpu_percent"] or 0 for row in rows), 3),
            "rss_bytes": sum(row["rss_bytes"] or 0 for row in rows),
            "pss_bytes": sum(row["pss_bytes"] or 0 for row in rows) if LINUX else None,
        },
        "processes": rows,
    }
    if reaped_before is not None:
        # Children alive at both samples are in the tree; short-lived ones (polls,
        # ssh) that exited and were reaped during the window count here (Linux only).
        gui["reaped_children_cpu_percent"] = round((second[root]["reaped_seconds"] - reaped_before) / elapsed * 100, 3)
    result = {
        "schema": "zerus-perf-v1",
        "client": "qt",
        "harness": "client/bench/sample-running-gui.py",
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platform": {"os": "linux" if LINUX else "macos", "label": args.label, "representative": True},
        "resources": {"running_gui": gui},
    }
    text = json.dumps(result, indent=2, ensure_ascii=False)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as handle:
            handle.write(text + "\n")
    else:
        print(text)


if __name__ == "__main__":
    main()
