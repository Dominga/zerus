#!/usr/bin/env python3
"""A fake `hgs swarm` for the organization tests. Reads `config.json` beside itself:
{"get": [reply, ...], "apply": [reply, ...]} with replies
{"exit": 0, "stdout": <object>, "stderr": "", "sleep": 0, "gate": "<name>"}; a
gate holds the reply until a file of that name appears beside the fake (at
most 20 s), so a test decides when a running call ends. Each call of a
subcommand takes its next reply and the last one repeats, so a list models
another writer changing the catalog between two calls. Appends
{"argv": [...], "stdin": "..."} to `log.ndjson` beside itself."""
import json
import os
import subprocess
import sys
import time

def adoption_receipt(kind, request, preview):
    import hashlib
    import uuid
    canonical = lambda value: json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    digest = lambda value: hashlib.sha256(canonical(value)).hexdigest()
    source = preview["source"]
    destination = preview["destination"]
    semantic = dict(source, configuration_revision="")
    request_hash = digest({"version": 1, "request_id": request["request_id"],
                           "direction": "initiator_to_empty_destination", "source": semantic,
                           "destination": destination, "destination_preview_sha256": preview["preview_sha256"]})
    binding = digest({"request_sha256": request_hash, "source_alias": "empty-peer",
                      "source_configuration_revision": source["configuration_revision"]})
    owner = digest({"native_ticket": request["native_ticket"], "native_generation": request["native_generation"],
                    "lease_id": request["lease_id"]})
    original = {"request_id": request["request_id"], "request_sha256": request_hash, "binding_sha256": binding,
                "owner_sha256": owner, "lease_id": request["lease_id"], "native_generation": request["native_generation"],
                "source_alias": "empty-peer", "source_configuration_revision": source["configuration_revision"],
                "source": source, "destination": destination, "source_catalog_raw_sha256": source["catalog_raw_sha256"],
                "request_raw_sha256": "0" * 64}
    if kind == "refused_not_started":
        return {"schema": 1, "phase": kind, "outcome": kind, "request_id": original["request_id"],
                "request_sha256": request_hash, "binding_sha256": binding, "original": original,
                "reasons": ["fixture refusal"], "product_effects": False}
    post = dict(destination, swarm_id=source["swarm_id"], catalog_digest=source["catalog_digest"],
                outer_store_sha256="8" * 64, primary=source["primary"], twins=source["twins"],
                empty_form="adopted_source_catalog", membership_epoch=str(uuid.uuid4()),
                membership_generation=max(1, destination["membership_generation"]), membership_sha256="7" * 64)
    main = {"original_source_catalog_sha256": source["catalog_raw_sha256"],
            "expected_source_catalog_digest": source["catalog_digest"],
            "destination_catalog_digest": source["catalog_digest"],
            "original_destination_store_sha256": destination["outer_store_sha256"],
            "destination_store_sha256": post["outer_store_sha256"],
            "destination_bindings_sha256": destination["bindings_sha256"],
            "destination_peers_empty": True, "parent_closure": True, "postread": True}
    component = lambda kind: {"component": kind, "source": source[kind], "destination": source[kind],
                              "destination_raw_sha256": source[kind]["raw_sha256"],
                              "destination_raw_len": source[kind]["raw_len"], "parent_closure": True, "postread": True}
    remote = {"schema": 1, "phase": "remote_committed", "outcome": "remote_committed",
              "request_id": original["request_id"], "request_sha256": request_hash, "binding_sha256": binding,
              "owner_sha256": owner, "original": original, "main_receipt": main,
              "primary_receipt": component("primary"), "twins_receipt": component("twins"),
              "post_destination": post, "destination_peers": {}, "destination_bindings": "self_only",
              "guard_cleared": True, "drive_status": "unsupported_uninitialized_without_reverse_peer",
              "membership_by_id_ready": False}
    if kind == "remote_committed":
        return remote
    return {"schema": 1, "phase": "settled", "request_id": original["request_id"],
            "request_sha256": request_hash, "binding_sha256": binding, "original": original, "remote": remote,
            "source_membership": {"original_source_store_sha256": source["outer_store_sha256"],
                                  "source_store_sha256": "6" * 64, "destination_node_id": destination["node_id"],
                                  "peer_added": "empty-peer", "postread": True},
            "destination_peers": {}, "destination_bindings": "self_only", "sync": "sync_peer_completed",
            "drive_status": "unsupported_uninitialized_without_reverse_peer", "membership_by_id_ready": False}


here = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
data = sys.stdin.read()
with open(os.path.join(here, "log.ndjson"), "a") as log:
    log.write(json.dumps({"argv": args, "stdin": data}) + "\n")
if args in (["swarm", "initialize", "--g3-status", "--json"], ["swarm", "initialize", "--g3", "--json"],
            ["swarm", "initialize", "--g3-bootstrap", "--json"]):
    action = {"--g3-status": "g3-status", "--g3": "g3-import", "--g3-bootstrap": "g3-bootstrap"}[args[2]]
elif args == ["machine", "ls"]:
    action = "machines"
elif len(args) == 3 and args[0] == "swarm" and args[1] in ("preview", "join"):
    action = args[1]
elif len(args) == 4 and args[0] == "swarm" and args[1] in ("adoption-preview", "adopt", "adoption-status") and args[3] == "--json":
    action = args[1]
elif len(args) == 2 and args[0] == "swarm" and args[1] in ("get", "apply", "sync", "resolve"):
    action = args[1]
else:
    sys.stderr.write("fake hgs: unexpected call\n")
    sys.exit(2)
with open(os.path.join(here, "config.json")) as config_file:
    settings = json.load(config_file)
    replies = settings[action]
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
if reply.get("gate"):
    deadline = time.time() + 20
    while not os.path.exists(os.path.join(here, reply["gate"])) and time.time() < deadline:
        time.sleep(0.02)
time.sleep(reply.get("sleep", 0))
if reply.get("hold_output"):
    code = """import os, sys, time
deadline = time.monotonic() + 20
while not os.path.exists(sys.argv[1]) and time.monotonic() < deadline:
    time.sleep(.02)
"""
    subprocess.Popen([sys.executable, "-c", code, os.path.join(here, reply["hold_output"])],
                     stdin=subprocess.DEVNULL,
                     env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "HOME": here, "TMPDIR": here})
if reply.get("complete_adoption_receipt"):
    output = adoption_receipt(reply["complete_adoption_receipt"], json.loads(data),
                              settings["adoption-preview"][0]["stdout"])
    for path, value in reply.get("receipt_mutation", {}).items():
        target = output
        parts = path.split(".")
        for part in parts[:-1]:
            target = target[part]
        target[parts[-1]] = value
    sys.stdout.write(json.dumps(output))
elif reply.get("stdout") is not None:
    output = dict(reply["stdout"])
    for key in reply.get("echo", []):
        output[key] = json.loads(data or "{}")[key]
    sys.stdout.write(json.dumps(output))
sys.stderr.write(reply.get("stderr", ""))
sys.exit(reply.get("exit", 0))
