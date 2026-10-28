#!/usr/bin/env python3
"""A fake `hgs` for the creation tests. Reads `config.json` beside itself:
{"help": "<help text>", "replies": {"<command>": {"exit": 0, "stdout": <object>, "stderr": ""}}}
where <command> is the first word after `@alias` and `--expect-machine M`.
Appends each argv to `log.ndjson` beside itself."""
import json, os, sys

here = os.path.dirname(os.path.abspath(__file__))
config = json.load(open(os.path.join(here, "config.json")))
argv = sys.argv[1:]
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps(argv) + "\n")
words = [a for a in argv if not a.startswith("@")]
if words[:1] == ["--expect-machine"]:
    words = words[2:]
if words[:1] == ["--help"]:
    sys.stdout.write(config.get("help", ""))
    sys.exit(0)
reply = config.get("replies", {}).get(words[0] if words else "", {"exit": 2, "stderr": "unknown"})
if "stdout" in reply:
    sys.stdout.write(json.dumps(reply["stdout"]))
sys.stderr.write(reply.get("stderr", ""))
sys.exit(reply.get("exit", 0))
