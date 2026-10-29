#!/usr/bin/env python3
"""A fake `hgs` for the voice composer tests. Reads `config.json` beside itself:
{"help": "<help text>", "reply": <object>}. Appends {"argv": [...], "stdin": "..."}
to `log.ndjson` beside itself."""
import json, os, sys

here = os.path.dirname(os.path.abspath(__file__))
config = json.load(open(os.path.join(here, "config.json")))
argv = sys.argv[1:]
data = sys.stdin.read()
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps({"argv": argv, "stdin": data}) + "\n")
if "--help" in argv:
    sys.stdout.write(config.get("help", ""))
else:
    sys.stdout.write(json.dumps(config.get("reply", {})))
