#!/usr/bin/env python3
"""A fake `hgs` for the client read tests; never the real CLI. Reads
`config.json` beside itself:

    {"fixtures": "/abs/tests/fixtures/reads",
     "replies": {"<argv joined by spaces>": {"file": "accounts.json" | "stdout": "...",
                 "repeat": n, "stderr": "...", "exit": 0, "sleep": seconds}}}

`repeat` writes `stdout` n times; `sleep` waits that many seconds before the
reply. An argv without a reply exits 2. Every call appends
{"argv": [...], "stdin": "..."} to `log.ndjson` beside itself."""
import json, os, sys, time

here = os.path.dirname(os.path.abspath(__file__))
config = json.load(open(os.path.join(here, "config.json")))
data = sys.stdin.read()
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps({"argv": sys.argv[1:], "stdin": data}) + "\n")
reply = config["replies"].get(" ".join(sys.argv[1:]))
if reply is None:
    sys.stderr.write("hgs: unknown command\n")
    sys.exit(2)
time.sleep(reply.get("sleep", 0))
if "file" in reply:
    sys.stdout.write(open(os.path.join(config["fixtures"], reply["file"])).read())
else:
    sys.stdout.write(reply.get("stdout", "") * reply.get("repeat", 1))
sys.stderr.write(reply.get("stderr", ""))
sys.stdout.flush()
sys.stderr.flush()
sys.exit(reply.get("exit", 0))
