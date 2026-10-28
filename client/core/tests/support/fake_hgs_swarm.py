#!/usr/bin/env python3
"""A fake `hgs swarm` for the organization tests. Reads `config.json` beside itself:
{"get": [reply, ...], "apply": [reply, ...]} with replies
{"exit": 0, "stdout": <object>, "stderr": "", "sleep": 0}. Each call of a
subcommand takes its next reply and the last one repeats, so a list models
another writer changing the catalog between two calls. Appends
{"argv": [...], "stdin": "..."} to `log.ndjson` beside itself."""
import json
import os
import sys
import time

here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
data = sys.stdin.read()
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps({"argv": args, "stdin": data}) + "\n")
if len(args) != 2 or args[0] != "swarm" or args[1] not in ("get", "apply"):
    sys.stderr.write("fake hgs: unexpected call\n")
    sys.exit(2)
action = args[1]
with open(os.path.join(here, "config.json")) as config_file:
    replies = json.load(config_file)[action]
counts_path = os.path.join(here, "counts.json")
counts = {}
if os.path.exists(counts_path):
    with open(counts_path) as counts_file:
        counts = json.load(counts_file)
n = counts.get(action, 0)
counts[action] = n + 1
with open(counts_path, "w") as counts_file:
    json.dump(counts, counts_file)
reply = replies[min(n, len(replies) - 1)]
time.sleep(reply.get("sleep", 0))
if reply.get("stdout") is not None:
    sys.stdout.write(json.dumps(reply["stdout"]))
sys.stderr.write(reply.get("stderr", ""))
sys.exit(reply.get("exit", 0))
