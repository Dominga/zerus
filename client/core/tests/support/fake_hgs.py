#!/usr/bin/env python3
"""A fake `hgs` for the zerus-client-core tests; never the real CLI.

Tests link or copy this file as `hgs` into a scratch folder next to a `config.json`.
On Windows, the copied `hgs.exe` launcher invokes it with Python:

    {"fixtures": "/abs/tests/fixtures",
     "hosts": {"": {"marker": true, "probe_exit": 0,
                    "runs": [[step, ...], ...]}, "alias": {...}}}

`hgs [@alias] -h` prints a help text with the watch-v1 marker unless `marker`
is false (or exits with `probe_exit`). `hgs [@alias] watch --json` plays the
steps of its run (the n-th invocation plays run n, the last run repeats):

    {"file": "basic.ndjson"}   write a fixture's lines
    {"lines": [frame, ...]}    write frames (a missing "seq" is numbered)
    {"sleep": seconds}
    {"await_file": "name"}     wait until the test creates that file
    {"exit": code}             exit at once
    {"serve": true}            read control lines until EOF, then exit 0
    {"serve": true, "until_file": "name"}   ... or exit 0 once that file exists

When the steps run out the process exits 0 (EOF on stdout). Every invocation,
control line and EOF is appended to `log.ndjson` with its time.

`hgs [@alias] swarm get` prints `swarm-get.json` beside this script, which a
test may rewrite; without it the call exits 2. For polling tests, `hgs [@alias]
ls --json` prints `ls-<alias or local>.json`, and `hgs [@alias] inspect NAME
--after N [--skip-processes]` prints `inspect-<alias or local>.json` with only
the events after N (the newest 100 for 0) and `cursor` at the newest event;
without the file either call exits 2.

`hgs [@alias] team OP --json` reads its JSON request from standard input, logs
it as `{"host", "team": OP, "request": ...}` and answers as
`team-OP-<alias or local>.json` beside this script says:
`{"stdout": object or text, "stderr": text, "exit": code, "sleep": seconds}`;
`"echo": true` adds the request's `request_id` to an object `stdout`. Without
the file the call exits 2 with "hgs: no reply".
`listing_wait_file` holds a listing at an acknowledged barrier; `listing_wait_after`
allows that many initial reads through before the barrier is armed.
"""
import json
import os
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
MARKER = "Event stream ABI: watch-v1"
PROCESS_RECEIPT_ID = f"{os.getpid()}-{time.monotonic_ns()}"


def receipt(event, host):
    # A receipt belongs to this original fake process, before shared logging.
    # A missing journal row does not mean its stdin never reached EOF.
    path = os.path.join(HERE, f"{event}-{PROCESS_RECEIPT_ID}.json")
    with open(path, "x", encoding="utf-8") as handle:
        json.dump({"schema": 1, "event": event, "host": host,
                   "pid": os.getpid(), "at": time.time()}, handle)


def log(entry):
    entry["at"] = time.time()
    line = json.dumps(entry) + "\n"
    # Several fake processes append at once. Windows emulates append mode with a
    # seek and a write, so concurrent entries can overwrite each other: hold an
    # exclusive lock on a separate file (Windows byte locks would block readers).
    with open(os.path.join(HERE, "log.lock"), "a+b") as lock:
        if os.name == "nt":
            import msvcrt
            lock.seek(0)
            msvcrt.locking(lock.fileno(), msvcrt.LK_LOCK, 1)
        else:
            import fcntl
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            with open(os.path.join(HERE, "log.ndjson"), "a", encoding="utf-8") as handle:
                handle.write(line)
                handle.flush()
        finally:
            if os.name == "nt":
                lock.seek(0)
                msvcrt.locking(lock.fileno(), msvcrt.LK_UNLCK, 1)


def previous_runs(host, args):
    count = 0
    with open(os.path.join(HERE, "log.ndjson"), encoding="utf-8") as handle:
        # A concurrent append may still be writing its last line. Match the
        # Rust fixture reader: only complete records establish an invocation.
        for line in handle.read().split("\n")[:-1]:
            entry = json.loads(line)
            if entry.get("host") == host and entry.get("args") == args:
                count += 1
    return count - 1


class Writer:
    def __init__(self):
        self.seq = 0

    def frame(self, frame):
        if isinstance(frame, str):
            self.raw(frame)
            return
        frame = dict(frame)
        self.seq = frame.setdefault("seq", self.seq + 1)
        frame.setdefault("v", 1)
        self.raw(json.dumps(frame, separators=(",", ":")))

    def raw(self, line):
        sys.stdout.write(line.rstrip("\n") + "\n")
        sys.stdout.flush()


def session_key(session):
    run = session.get("run_id") or ""
    return "run:" + run if run else "name:" + session.get("name", "")


def serve(host, writer, until_file=None):
    if until_file:
        # Serve on a thread; the stream ends (exit 0) once the file exists.
        reader = threading.Thread(target=serve, args=(host, writer), daemon=True)
        reader.start()
        path = os.path.join(HERE, until_file)
        while reader.is_alive() and not os.path.exists(path):
            time.sleep(0.01)
        log({"host": host, "exit": 0})
        sys.stdout.flush()
        os._exit(0)
    receipt("serve-ready", host)
    for line in sys.stdin:
        try:
            control = json.loads(line)
        except ValueError:
            control = {"invalid": line}
        log({"host": host, "stdin": control})
        if control.get("op") == "subscribe" and control.get("topic") == "activity":
            key = session_key(control.get("session") or {})
            writer.frame({"t": "events", "topic": "activity", "key": key,
                          "events": [{"type": "Stop", "seq": 1, "at": 1787840000.0, "detail": "Synthetic reply"}],
                          "cursor": 1})
        elif control.get("op") == "ping":
            writer.frame({"t": "pong", "id": control.get("id")})
        elif control.get("op") == "subscribe" and control.get("topic") == "git":
            path = control.get("path")
            writer.frame({"t": "state", "topic": "git", "key": path, "rev": 1,
                          "value": {"state": "ok", "root": path, "branch": "fixture",
                                    "changed_files": 3, "ahead": 2, "behind": 1,
                                    "remote_state": "verified", "remote_age_seconds": 0}})
    receipt("stdin-eof", host)
    log({"host": host, "eof": True})
    return 0


def watch(host, spec, config):
    runs = spec.get("runs") or [[]]
    steps = runs[min(previous_runs(host, ["watch", "--json"]), len(runs) - 1)]
    writer = Writer()
    for step in steps:
        if "file" in step:
            with open(os.path.join(config["fixtures"], step["file"]), encoding="utf-8") as handle:
                for line in handle:
                    writer.raw(line)
        elif "lines" in step:
            for frame in step["lines"]:
                writer.frame(frame)
        elif "sleep" in step:
            time.sleep(step["sleep"])
        elif "await_file" in step:
            path = os.path.join(HERE, step["await_file"])
            while not os.path.exists(path):
                time.sleep(0.01)
        elif "exit" in step:
            log({"host": host, "exit": step["exit"]})
            return step["exit"]
        elif step.get("serve"):
            return serve(host, writer, step.get("until_file"))
    return 0


def main():
    args = sys.argv[1:]
    host = ""
    if args and args[0].startswith("@"):
        host, args = args[0][1:], args[1:]
    with open(os.path.join(HERE, "config.json"), encoding="utf-8") as handle:
        config = json.load(handle)
    log({"host": host, "args": args})
    spec = config.get("hosts", {}).get(host)
    if spec is None:
        sys.stderr.write("hgs: unknown computer\n")
        return 255
    if args in (["-h"], ["--help"]):
        if spec.get("probe_exit", 0):
            return spec["probe_exit"]
        print("Usage: hgs [@host] <command>")
        if spec.get("marker", True):
            print("  " + MARKER)
        return 0
    if args == ["watch", "--json"]:
        return watch(host, spec, config)
    reply = os.path.join(HERE, "%s-" + (host or "local") + ".json")
    # Task 235: the legacy listing names --local --no-archives.
    if args in (["ls", "--json"], ["ls", "--json", "--local", "--no-archives"]):
        if spec.get("listing_wait_file") and previous_runs(host, args) >= spec.get("listing_wait_after", 0):
            log({"host": host, "listing_wait": spec["listing_wait_file"]})
            while not os.path.exists(os.path.join(HERE, spec["listing_wait_file"])):
                time.sleep(0.01)
        if not os.path.exists(reply % "ls"):
            return 2
        with open(reply % "ls", encoding="utf-8") as handle:
            sys.stdout.write(handle.read())
        return spec.get("listing_exit", 0)
    if args and args[0] == "inspect":
        if not os.path.exists(reply % "inspect"):
            return 2
        with open(reply % "inspect", encoding="utf-8") as handle:
            data = json.load(handle)
        after = int(args[args.index("--after") + 1]) if "--after" in args else 0
        events = data.get("events", [])
        newest = max([event.get("seq", 0) for event in events] + [0])
        data["events"] = [event for event in events if event.get("seq", 0) > after][-100:]
        data["cursor"] = newest
        if "--skip-processes" in args:
            data.pop("processes", None)
            data.pop("process_work", None)
        sys.stdout.write(json.dumps(data))
        return 0
    if len(args) == 4 and args[0] == "team" and args[1] == "catalog" and args[3] == "--json":
        body = sys.stdin.read()
        try:
            request = json.loads(body)
        except ValueError:
            request = body
        log({"host": host, "team": "catalog " + args[2], "request": request})
        path = reply % ("team-catalog-" + args[2])
        if not os.path.exists(path):
            sys.stderr.write("hgs: no reply" + chr(10))
            return 2
        with open(path, encoding="utf-8") as handle:
            answer = json.load(handle)
        if answer.get("sleep"):
            time.sleep(answer["sleep"])
        out = answer.get("stdout")
        if isinstance(out, dict) and answer.get("echo") and isinstance(request, dict):
            out = dict(out, request_id=request.get("request_id"))
        if out is not None:
            sys.stdout.write(out if isinstance(out, str) else json.dumps(out))
            sys.stdout.flush()
        if answer.get("stderr"):
            sys.stderr.write(answer["stderr"])
        return answer.get("exit", 0)
    if len(args) == 3 and args[0] == "team" and args[2] == "--json":
        body = sys.stdin.read()
        try:
            request = json.loads(body)
        except ValueError:
            request = body
        log({"host": host, "team": args[1], "request": request})
        path = reply % ("team-" + args[1])
        if not os.path.exists(path):
            sys.stderr.write("hgs: no reply" + chr(10))
            return 2
        with open(path, encoding="utf-8") as handle:
            answer = json.load(handle)
        if answer.get("sleep"):
            time.sleep(answer["sleep"])
        out = answer.get("stdout")
        if isinstance(out, dict) and answer.get("echo") and isinstance(request, dict):
            out = dict(out, request_id=request.get("request_id"))
        if out is not None:
            sys.stdout.write(out if isinstance(out, str) else json.dumps(out))
            sys.stdout.flush()
        if answer.get("stderr"):
            sys.stderr.write(answer["stderr"])
        return answer.get("exit", 0)
    if args == ["swarm", "get"]:
        path = os.path.join(HERE, "swarm-get.json")
        if not os.path.exists(path):
            return 2
        with open(path, encoding="utf-8") as handle:
            sys.stdout.write(handle.read())
        return 0
    return 2


if __name__ == "__main__":
    sys.exit(main())
