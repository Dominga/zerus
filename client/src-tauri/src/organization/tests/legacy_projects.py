#!/usr/bin/env python3
"""Private, guarded CLI stand-in for the actual Organization migration worker."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

here = Path(os.path.abspath(__file__)).parent
args = sys.argv[1:]
data = sys.stdin.read()
with (here / "log.ndjson").open("a") as log:
    log.write(json.dumps({"argv": args, "stdin": data}) + "\n")
config = json.loads((here / "legacy-config.json").read_text())
words = args[:]
alias = words.pop(0)[1:] if words and words[0].startswith("@") else ""
if words == ["--help"]:
    print("guarded-create-v1" if config.get("support", True) else "old CLI")
    sys.exit(0)
expected = config["peer"] if alias else config["owner"]
if words[:2] != ["--expect-machine", expected]:
    sys.stderr.write("private fixture: wrong target\n")
    sys.exit(2)
words = words[2:]
counts_file = here / "legacy-counts.json"
counts = json.loads(counts_file.read_text()) if counts_file.exists() else {}
action = "projects" if words == ["project", "ls", "--json"] else "machines" if words == ["machine", "ls"] else words[1] if words[:1] == ["swarm"] else "invalid"
counts[action] = counts.get(action, 0) + 1
counts_file.write_text(json.dumps(counts))
snapshot = None
if action == "get":
    snapshot = (here / "legacy-state.json").read_text()
    (here / f"get-{counts[action]}-read").touch()
gate = config.get("gates", {}).get(f"{action}-{counts[action]}")
if gate:
    deadline = time.monotonic() + 20
    while not (here / gate).exists() and time.monotonic() < deadline:
        time.sleep(.01)
if counts[action] in config.get("failures", {}).get(action, []):
    sys.stderr.write("private read failure /srv/retained-source must not enter report\n")
    sys.exit(2)
if action == "machines":
    revision = "b" if config.get("changed_config") and counts.get("projects") else "a"
    print(json.dumps({"local": "fixture", "revision": revision * 64,
                      "machines": [{"alias": "fixture-peer", "enabled": True}]}))
elif action == "projects":
    if config.get("hold_overflow"):
        child = """import sys,time
from pathlib import Path
p=Path(sys.argv[1]); (p/'pipes-held').touch()
deadline=time.monotonic()+20
while not (p/'release-pipes').exists() and time.monotonic()<deadline: time.sleep(.01)
"""
        subprocess.Popen([sys.executable, "-c", child, str(here)], stdin=subprocess.DEVNULL,
                         env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "HOME": str(here), "TMPDIR": str(here)})
        print(json.dumps([{"name":"Legacy", "dir":"/srv/legacy", "ignored":"x"*(2*1024*1024)}]))
    else:
        print(json.dumps(config["rows"]))
elif action == "get":
    print(snapshot)
elif action == "apply":
    original = json.loads((here / "legacy-state.json").read_text())
    request = json.loads(data)
    if config.get("land", True):
        order = original["organization"]["project_order"]
        original["organization"] = request["desired"]
        original["organization"]["project_order"] = order
        original["digest"] = "c" * 64
        path = here / "legacy-state.new"
        path.write_text(json.dumps(original))
        path.replace(here / "legacy-state.json")
    if config.get("unknown"):
        sys.stderr.write("private project name /srv/retained-source must not enter report\n")
        sys.exit(2)
    print(json.dumps(original))
else:
    sys.exit(2)
