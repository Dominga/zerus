#!/usr/bin/env python3
"""Run real Linux worktree fixtures with an owned, inspectable process inventory.

The production guard remains unchanged: inaccessible processes retain folders.
The whole fixture, including its live blockers and private tmux, shares a fresh
PID/mount namespace. No host process is hidden from just an individual hgs call.
Hosted disposable runners use sudo solely to create that namespace, then drop
back to the original uid/gid before Python or fixture code runs. Local runs use
an unprivileged user namespace and drop all capabilities. Admission errors fail
the job; there is no uncontained retry or skipped positive test.
"""
import errno
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import unittest


def tool(name):
    path = shutil.which(name)
    if not path:
        raise SystemExit(f"Worktree fixture isolation requires {name}")
    return path


def main():
    if sys.platform != "linux":
        raise SystemExit("This process namespace fixture requires Linux")
    if sys.argv[1:2] != ["--inside"]:
        unshare = tool("unshare")
        flags = ["--mount", "--pid", "--fork", "--mount-proc", "--kill-child=KILL"]
        if os.environ.get("GITHUB_ACTIONS") == "true":
            # Only an ordinary Linux hosted test job enters this path. Preserve
            # its selected test binary and tools; execute all fixture code as uid.
            command = [tool("sudo"), "-n", "-E", "--", unshare, *flags,
                       tool("setpriv"), f"--reuid={os.getuid()}", f"--regid={os.getgid()}",
                       "--clear-groups", "--bounding-set=-all", "--inh-caps=-all",
                       "--ambient-caps=-all",
                       tool("env"), "PATH=" + os.environ.get("PATH", ""),
                       "HOME=" + os.environ.get("HOME", "")]
        else:
            command = [unshare, "--user", "--map-root-user", *flags,
                       tool("setpriv"), "--bounding-set=-all", "--inh-caps=-all",
                       "--ambient-caps=-all"]
        command += [sys.executable, str(Path(__file__).resolve()), "--inside", *sys.argv[1:]]
        os.execvpe(command[0], command, os.environ)

    if os.getpid() != 1:
        raise SystemExit("Worktree fixture did not enter its own PID namespace")
    status = dict(line.split(":", 1) for line in Path("/proc/self/status").read_text().splitlines() if ":" in line)
    if any(int(status[key], 16) for key in ("CapEff", "CapPrm", "CapAmb")):
        raise SystemExit("Worktree fixture must run without inspection capabilities")
    # PID 1 handles termination explicitly; namespace exit also reaps detached
    # fixture descendants. No signal here targets a host process.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    child = subprocess.Popen([sys.executable, "-I", "-c",
        "import ctypes,time; assert ctypes.CDLL(None).prctl(4,0,0,0,0)==0; print('ready',flush=True); time.sleep(20)"],
        stdout=subprocess.PIPE, text=True)
    try:
        if child.stdout.readline().strip() != "ready" or child.poll() is not None:
            raise SystemExit("Worktree namespace process preflight did not start")
        try:
            os.readlink(f"/proc/{child.pid}/cwd")
        except OSError as error:
            if error.errno not in (errno.EACCES, errno.EPERM):
                raise
        else:
            raise SystemExit("Worktree namespace bypasses inaccessible-process protection")
    finally:
        child.terminate()
        child.wait(timeout=5)
        child.stdout.close()
    print("Worktree process scope: private PID namespace, no capabilities, inaccessible-process control passed", flush=True)
    unittest.main(module=None, argv=[sys.argv[0], "-v", *sys.argv[2:]])


if __name__ == "__main__":
    main()
