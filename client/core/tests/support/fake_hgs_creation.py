#!/usr/bin/env python3
"""A fake `hgs` for the creation tests. Reads `config.json` beside itself:
{"help": "<help text>", "stamp": true, "replies": {"<command>": {"exit": 0, "stdout": <object>, "stderr": ""}}}
where <command> is the first two words after `@alias` and `--expect-machine M`
(such as "worktrees create"), else the first word alone. A reply may also have
"sleep" (seconds before it answers), "echo" ({"key": "--flag"}: the argument
after --flag goes into stdout's key, or a dotted path such as sessions.0.launch_id,
for receipts that name a fresh identity; later reads reuse that id and the
created worktree's branch/path) and "raw" (text written as it is,
"repeat" times). Appends each argv to `log.ndjson` beside itself, and
[unix_ms, argv] to `stamped.ndjson` when stamp is true."""
import json, os, sys, time

here = os.path.dirname(os.path.abspath(__file__))
config = json.load(open(os.path.join(here, "config.json")))
argv = sys.argv[1:]
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps(argv) + "\n")
if config.get("stamp"):
    with open(os.path.join(here, "stamped.ndjson"), "a") as log:
        log.write(json.dumps([int(time.time() * 1000), argv]) + "\n")
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
        remembered = {"--launch-id": "last-launch-id", "--last-worktree-branch": "last-worktree-branch", "--last-worktree-path": "last-worktree-path"}
        if flag not in argv:
            if flag not in remembered or not os.path.exists(os.path.join(here, remembered[flag])):
                continue
            with open(os.path.join(here, remembered[flag])) as saved:
                value = saved.read()
        else:
            value = argv[argv.index(flag) + 1]
        if flag == "--launch-id":
            with open(os.path.join(here, "last-launch-id"), "w") as saved:
                saved.write(value)
        elif flag == "--branch":
            with open(os.path.join(here, "last-worktree-branch"), "w") as saved:
                saved.write(value)
        elif flag == "--destination":
            with open(os.path.join(here, "last-worktree-path"), "w") as saved:
                saved.write(value)
        target = stdout
        parts = key.split(".")
        for part in parts[:-1]:
            target = target[int(part)] if part.isdigit() else target[part]
        target[parts[-1]] = value
    sys.stdout.write(json.dumps(stdout))
sys.stderr.write(reply.get("stderr", ""))
sys.exit(reply.get("exit", 0))
