#!/usr/bin/env python3
"""A fake `hgs` for the client action tests. Reads `config.json` beside itself:
{"reply": {"exit": 0, "stdout": <object or "echo_interrupt">, "stderr": "", "sleep": 0, "log_env": []}}
Appends {"argv": [...], "stdin": "...", "env": {...}} to `log.ndjson` beside itself."""
import json, os, sys, time

here = os.path.dirname(os.path.abspath(__file__))
config = json.load(open(os.path.join(here, "config.json")))["reply"]
data = sys.stdin.read()
entry = {"argv": sys.argv[1:], "stdin": data}
if config.get("log_env"):
    entry["env"] = {k: os.environ.get(k) for k in config["log_env"]}
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps(entry) + "\n")
time.sleep(config.get("sleep", 0))
out = config.get("stdout")
if out == "echo_interrupt":
    request = json.loads(data)
    args = [a for a in sys.argv[1:] if not a.startswith("@")]
    out = {"status": "submitted", "request_id": request["request_id"], "name": args[1],
           "run_id": request["expected_run_id"], "conversation_id": request["expected_conversation_id"]}
if out is not None:
    sys.stdout.write(json.dumps(out))
sys.stderr.write(config.get("stderr", ""))
sys.exit(config.get("exit", 0))
