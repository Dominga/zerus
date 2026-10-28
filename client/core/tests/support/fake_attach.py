#!/usr/bin/env python3
"""A fake `hgs a NAME --existing` for the Terminal tests. It runs in the
pseudoterminal, prints one JSON line with its arguments, environment and
size, then answers commands, one per line on its input:
  size        prints "size COLS ROWS"
  exit N      exits with N
  flood N     writes N bytes of "x" and then "done"
  child       starts a sleeping child in its process group, prints "child PID"
  hang        stops reading its input
  echo TEXT   prints "echo TEXT"
"""
import json, os, subprocess, sys, termios, tty

def say(text):
    sys.stdout.write(text + "\r\n")
    sys.stdout.flush()

# Raw mode: no echo, no line discipline translations of the output.
tty.setraw(sys.stdin.fileno())
size = os.get_terminal_size(sys.stdout.fileno())
fds = sorted(int(fd) for fd in os.listdir("/proc/self/fd")) if os.path.isdir("/proc/self/fd") else None
say(json.dumps({"argv": sys.argv[1:], "cols": size.columns, "rows": size.lines, "fds": fds,
                "env": {k: os.environ.get(k) for k in
                        ["TMUX", "TMUX_PANE", "HGS_SESSION", "HGS_RUN_ID", "HGS_TAB", "TERM", "COLORTERM"]}}))
line = b""
while True:
    byte = os.read(sys.stdin.fileno(), 1)
    if not byte:
        sys.exit(0)
    if byte not in (b"\r", b"\n"):
        line += byte
        continue
    command, _, rest = line.decode().partition(" ")
    line = b""
    if command == "size":
        size = os.get_terminal_size(sys.stdout.fileno())
        say(f"size {size.columns} {size.lines}")
    elif command == "exit":
        sys.exit(int(rest))
    elif command == "flood":
        left = int(rest)
        while left:
            chunk = min(left, 65536)
            os.write(sys.stdout.fileno(), b"x" * chunk)
            left -= chunk
        say("done")
    elif command == "child":
        child = subprocess.Popen(["sleep", "60"])
        say(f"child {child.pid}")
    elif command == "hang":
        say("hanging")
        while True:
            os.pause() if hasattr(os, "pause") else None
    elif command == "echo":
        say(f"echo {rest}")
