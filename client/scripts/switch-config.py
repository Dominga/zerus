#!/usr/bin/env python3
"""Write the Tauri configuration of the Mac switch bundle.

tauri.switch.conf.json with Zerus' version from VERSION and every path made
absolute, so `tauri build --config OUTPUT` reads the same files wherever
OUTPUT lives (docs/macos-distribution.md#tauri-client-switch).

    switch-config.py OUTPUT
"""
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
TAURI = ROOT / "client/src-tauri"


def absolute(path):
    return str((TAURI / path).resolve())


def config(version):
    data = json.loads((TAURI / "tauri.switch.conf.json").read_text())
    data["version"] = version
    bundle = data["bundle"]
    bundle["resources"] = {absolute(source): target for source, target in bundle["resources"].items()}
    mac = bundle["macOS"]
    mac["infoPlist"] = absolute(mac["infoPlist"])
    mac["files"] = {target: absolute(source) for target, source in mac["files"].items()}
    return data


def main(argv):
    if len(argv) != 2:
        print(__doc__.split("\n\n")[-1], file=sys.stderr)
        return 2
    output = Path(argv[1])
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(config((ROOT / "VERSION").read_text().strip()), indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
