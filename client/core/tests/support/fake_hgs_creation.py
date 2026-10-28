#!/usr/bin/env python3
"""A fake `hgs` for the creation tests. Reads `config.json` beside itself:
{"help": "<help text>", "replies": {"<command>": {"exit": 0, "stdout": <object>, "stderr": ""}}}
where <command> is the first two words after `@alias` and `--expect-machine M`
(such as "worktrees create"), else the first word alone. A reply may also have
"sleep" (seconds before it answers), "echo" ({"key": "--flag"}: the argument
after --flag goes into stdout's key, for receipts that name a fresh identity)
and "raw" (text written as it is, "repeat" times).
Appends each argv to `log.ndjson` beside itself."""
import json, os, sys, time

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
replies = config.get("replies", {})
reply = replies.get(" ".join(words[:2])) or replies.get(words[0] if words else "", {"exit": 2, "stderr": "unknown"})
time.sleep(reply.get("sleep", 0))
if "raw" in reply:
    sys.stdout.write(reply["raw"] * reply.get("repeat", 1))
elif "stdout" in reply:
    stdout = reply["stdout"]
    for key, flag in reply.get("echo", {}).items():
        stdout[key] = argv[argv.index(flag) + 1]
    sys.stdout.write(json.dumps(stdout))
sys.stderr.write(reply.get("stderr", ""))
sys.exit(reply.get("exit", 0))
