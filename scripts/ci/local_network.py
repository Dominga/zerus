"""Retain one unprivileged outbound network with its original test namespace."""
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import time


def read_ready(fd, timeout):
    if not select.select([fd], [], [], timeout)[0] or not os.read(fd, 1):
        raise RuntimeError("private network did not become ready")


class Network:
    def __init__(self, binary, log):
        self.binary, self.log = binary, log
        self.fds, self.helper = [], None
        self.release_entered = False

    def pipe(self):
        pair = os.pipe()
        self.fds.extend(pair)
        return pair

    def close(self, fd):
        if fd in self.fds:
            os.close(fd)
            self.fds.remove(fd)

    def before_spawn(self, command):
        self.info, self.info_write = self.pipe()
        self.block, self.release = self.pipe()
        start = command.index("bwrap") + 1
        command[start:start] = ["--info-fd", str(self.info_write), "--block-fd", str(self.block)]
        return (self.info_write, self.block)

    def connect(self):
        self.close(self.info_write)
        self.close(self.block)
        data, deadline = b"", time.monotonic() + 10
        while time.monotonic() < deadline:
            if not select.select([self.info], [], [], max(0, deadline - time.monotonic()))[0]:
                break
            part = os.read(self.info, 4096)
            if not part:
                break
            data += part
            try:
                info = json.loads(data)
                break
            except json.JSONDecodeError:
                continue
        else:
            info = None
        if not data:
            raise RuntimeError("original namespace was not created")
        try:
            info = json.loads(data)
            child = int(info["child-pid"])
        except (KeyError, ValueError, json.JSONDecodeError) as error:
            raise RuntimeError("original namespace identity unavailable") from error
        ready, ready_write = self.pipe()
        exit_read, self.exit_write = self.pipe()
        self.helper = subprocess.Popen([
            str(self.binary), "--configure", "--disable-host-loopback", "--enable-sandbox", "--enable-seccomp",
            f"--ready-fd={ready_write}", f"--exit-fd={exit_read}", str(child), "tap0"],
            pass_fds=(ready_write, exit_read), stdin=subprocess.DEVNULL, stdout=self.log, stderr=subprocess.STDOUT,
            start_new_session=True, env={"PATH": "/usr/bin:/bin"})
        self.close(ready_write)
        self.close(exit_read)
        read_ready(ready, 10)
        if self.helper.poll() is not None:
            raise RuntimeError("original network helper exited before release")
        self.release_entered = True
        os.write(self.release, b"1")
        self.close(self.release)

    def finish(self):
        for fd in self.fds[:]:
            self.close(fd)
        if self.helper is not None:
            try:
                self.helper.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(self.helper.pid, signal.SIGKILL)
                self.helper.wait(timeout=5)
