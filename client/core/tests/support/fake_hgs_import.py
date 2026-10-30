#!/usr/bin/env python3
"""Isolated desktop-import wire peer; reads only this test's synthetic root."""
import hashlib
import json
import os
from pathlib import Path
import sys
import time

root = Path(os.environ["TEST_IMPORT_ROOT"])
options = json.loads((root / "options.json").read_text())
args = sys.argv[1:]
with (root / "calls.jsonl").open("a") as log:
    log.write(json.dumps({"args": args, "config": os.environ.get("HGS_CONFIG_DIR"),
                          "state": os.environ.get("HGS_STATE_DIR"),
                          "session": os.environ.get("HGS_SESSION"),
                          "run": os.environ.get("HGS_RUN_ID")}) + "\n")
abi = "desktop-import-v1"
config = options.get("configuration", "a" * 64)

if args == ["--help"]:
    print(options.get("help", "Usage: hgs\n  Desktop import ABI: " + abi))
elif args == ["desktop", "status", "--json"]:
    print(json.dumps({"abi": "desktop-v1", "configuration": config,
                      "running": False, "writers_enabled": False}))
elif args[:4] == ["desktop", "import-status", "--json", "--import-id"]:
    import_id = args[4]
    record_path = root / (import_id + ".json")
    result = dict(abi=abi, configuration=config, import_id=import_id,
                  record_present=record_path.exists() and not options.get("absent"))
    if result["record_present"]:
        record_bytes = record_path.read_bytes()
        record = json.loads(record_bytes)
        result.update(evidence=record["evidence"], pages=len(record["pages"]),
                      import_record=dict(version=1, sha256=hashlib.sha256(record_bytes).hexdigest()))
    result.update(options.get("status_patch", {}))
    print(json.dumps(result))
elif args == ["desktop", "import-evidence", "--json"]:
    if options.get("noisy"):
        sys.stderr.write("x" * (16384 + 1))
        sys.stderr.flush()
        time.sleep(10)
        sys.exit(1)
    pipe = sys.stdin.buffer

    def header():
        line = pipe.readline(4097)
        assert len(line) <= 4096 and line.endswith(b"\n")
        return json.loads(line)

    start = header()
    assert start["abi"] == abi and start["configuration"] == config
    assert start["source"] == "com.hgdev.hgs-tray" and start["replay"] is False
    snapshots, pages, indexes = {}, [], {}
    total = 0
    try:
        while True:
            item = header()
            length = item.get("bytes", item.get("reference", {}).get("bytes"))
            data = pipe.read(length)
            assert len(data) == length and pipe.read(1) == b"\n"
            total += length
            assert total <= start["max_bytes"]
            digest = hashlib.sha256(data).hexdigest()
            if item["t"] == "record":
                record = json.loads(data)
                assert record["configuration"] == config and record["import_id"] == start["import_id"]
                assert record["evidence"] == "staged" and record["automatic_approvals"] is False
                assert record["pages"] == pages and pipe.read(1) == b""
                assert len(snapshots) <= 1024 and len(pages) <= 1024
                (root / (record["import_id"] + ".json")).write_bytes(data)
                result = dict(abi=abi, published=True, evidence="staged", configuration=config,
                              import_id=record["import_id"], bytes=total,
                              snapshots=len(snapshots), pages=len(pages),
                              import_record=dict(version=1, sha256=digest))
                break
            assert item["reference"] == dict(sha256=digest, bytes=length)
            if item["t"] == "snapshot":
                assert digest not in snapshots and length <= 1048576
                json.loads(data)
                snapshots[digest] = length
            else:
                assert item["t"] == "page" and item["reference"] not in pages
                page = json.loads(data)
                ref = page["snapshot"]
                assert snapshots[ref["sha256"]] == ref["bytes"]
                family = page["family"]
                assert page["index"] == indexes.get(family, 0)
                indexes[family] = page["index"] + 1
                pages.append(item["reference"])
    except (AssertionError, KeyError, json.JSONDecodeError):
        sys.exit(1)
    if options.get("lost"):
        sys.exit(1)
    result.update(options.get("reply_patch", {}))
    print(json.dumps(result))
else:
    sys.exit(2)
