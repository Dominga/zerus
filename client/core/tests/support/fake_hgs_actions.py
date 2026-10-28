#!/usr/bin/env python3
"""A fake `hgs` for the client action tests. Reads `config.json` beside itself:
{"reply": {"exit": 0, "stdout": <object, "echo_interrupt", "echo_send", "echo_stage" or "echo_session_action" (with "receipt_status", "receipt_error")>, "stdout_text": "<raw text instead of stdout>", "stderr": "", "sleep": 0, "log_env": [], "read_stdin": true, "linger": 0},
 "replies": [{"match": [<args after an @alias>], ...the same keys as "reply"}]}
The first entry of the optional "replies" whose "match" is a prefix of the arguments
(without a leading @alias) answers; otherwise "reply" does.
Appends {"argv": [...], "stdin": "...", "env": {...}} to `log.ndjson` beside itself."""
import json, os, sys, time

here = os.path.dirname(os.path.abspath(__file__))
whole = json.load(open(os.path.join(here, "config.json")))
config = whole["reply"]
_args = [a for a in sys.argv[1:] if not a.startswith("@")] if sys.argv[1:2] and sys.argv[1].startswith("@") else sys.argv[1:]
for _candidate in whole.get("replies", []):
    _match = _candidate["match"]
    if _args[:len(_match)] == _match:
        config = _candidate
        break
data = sys.stdin.read() if config.get("read_stdin", True) else ""
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
elif out in ("echo_send", "echo_stage"):
    request = json.loads(data)
    args = [a for a in sys.argv[1:] if not a.startswith("@")]
    out = {"status": "submitted" if out == "echo_send" else "staged", "request_id": request["request_id"],
           "name": args[1], "run_id": request["expected_run_id"],
           "conversation_id": request["expected_conversation_id"]}
    if "agent_id" in request:
        out["agent_id"] = request["agent_id"]
if out == "echo_session_action":
    # Echoes a session-action request as a receipt; "receipt_status" is its status.
    request = json.loads(data)
    args = [a for a in sys.argv[1:] if not a.startswith("@")]
    out = {"status": config.get("receipt_status", "completed"), "request_id": request["request_id"],
           "name": args[1], "run_id": request["expected_run_id"],
           "conversation_id": request["expected_conversation_id"]}
    if "receipt_error" in config:
        out["error"] = config["receipt_error"]
if out is not None:
    sys.stdout.write(json.dumps(out))
if "stdout_text" in config:
    sys.stdout.write(config["stdout_text"])
sys.stderr.write(config.get("stderr", ""))
sys.stdout.flush()
sys.stderr.flush()
if config.get("linger"):
    # A background grandchild that keeps the inherited pipes open after we exit.
    if os.fork() == 0:
        time.sleep(config["linger"])
        os._exit(0)
sys.exit(config.get("exit", 0))
