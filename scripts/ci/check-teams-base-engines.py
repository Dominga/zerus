#!/usr/bin/env python3
"""Exercise both real Teams engines on one store, including an open old handle.

Run under the normal heavy wrapper with a private HOME/TMPDIR/TMUX_TMPDIR and
CARGO_TARGET_DIR under this checkout. Product rows come only from each image's
operation writers. The previous source receives only the committed test module.
This is a test image, never an alternate way to invoke a person-only CLI action.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import time

PREVIOUS = "34c231f26399229828f869dfdfbac1cb4ab0cd4e"
ROOT = Path(__file__).resolve().parents[2]


def image(source, target, label, evidence):
    command = ["cargo", "+1.99.1", "test", "--locked", "-j", "4", "--bin", "hgs", "--no-run", "--message-format=json"]
    env = os.environ.copy()
    if label == "previous":
        # Cargo can share output filenames across two path-package roots.
        # Never let the previous source overwrite the current image's cache.
        env["CARGO_TARGET_DIR"] = str(target / "previous-target")
    result = subprocess.run(command, cwd=source, env=env, text=True, capture_output=True)
    (evidence / (label + "-build.jsonl")).write_text(result.stdout)
    (evidence / (label + "-build.stderr")).write_text(result.stderr)
    result.check_returncode()
    outputs = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")]
    binaries = [row["executable"] for row in outputs if row.get("reason") == "compiler-artifact" and row.get("executable") and row.get("profile", {}).get("test") and row["target"]["name"] == "hgs"]
    assert len(binaries) == 1, binaries
    destination = target / (label + "-engine")
    shutil.copy2(binaries[0], destination)
    return destination


def step(binary, selector, evidence, env):
    command = [str(binary), "--ignored", "--exact", selector, "--nocapture", "--test-threads=1"]
    with (evidence / (selector.rsplit("::", 1)[-1] + ".log")).open("w") as output:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT, timeout=60)
    result.check_returncode()
    text = (evidence / (selector.rsplit("::", 1)[-1] + ".log")).read_text()
    assert "test result: ok. 1 passed;" in text, "The selected probe did not execute exactly once"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--reuse-previous", action="store_true")
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    target = Path(os.environ["CARGO_TARGET_DIR"]).resolve()
    assert target.is_relative_to(ROOT) and target != ROOT
    private = target / "wp165-r2/engine-probe"
    private.mkdir(parents=True, exist_ok=True, mode=0o700)
    previous = private / "previous-engine"
    fixture = ROOT / "tests/fixtures/teams/base_engine_previous.rs"
    fixture_hash = hashlib.sha256(fixture.read_bytes()).hexdigest()
    seal_path = private / "previous-image-seal.json"
    seal = json.loads(seal_path.read_text()) if seal_path.exists() else {}
    reuse = (args.reuse_previous and previous.exists() and seal.get("source_commit") == PREVIOUS and seal.get("fixture_sha256") == fixture_hash
             and seal.get("image_sha256") == hashlib.sha256(previous.read_bytes()).hexdigest())
    if not reuse:
        source = private / "previous-source"
        if not source.exists():
            source.mkdir()
            archive = subprocess.Popen(["git", "archive", PREVIOUS], cwd=ROOT, stdout=subprocess.PIPE)
            try:
                with tarfile.open(fileobj=archive.stdout, mode="r|") as entries:
                    entries.extractall(source, filter="data")
            finally:
                archive.stdout.close()
            assert archive.wait() == 0
        test = source / "src/teams/ops/tests/base_engine_previous.rs"
        shutil.copy2(fixture, test)
        module = source / "src/teams/ops/tests/mod.rs"
        if "mod base_engine_previous;" not in module.read_text():
            module.write_text(module.read_text() + "\nmod base_engine_previous;\n")
        previous = image(source, private, "previous", evidence)
        seal = {"source_commit": PREVIOUS, "fixture_sha256": fixture_hash, "image_sha256": hashlib.sha256(previous.read_bytes()).hexdigest()}
        seal_path.write_text(json.dumps(seal, indent=2) + "\n")
    (evidence / "previous-image-seal.json").write_text(json.dumps(seal, indent=2) + "\n")
    assert previous.exists()
    current = image(ROOT, private, "current", evidence)
    assert hashlib.sha256(previous.read_bytes()).digest() != hashlib.sha256(current.read_bytes()).digest(), "Previous/current test images collided"
    identity = {"previous_commit": PREVIOUS, "current_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(), "images": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in [previous, current]}}
    (evidence / "engine-images.json").write_text(json.dumps(identity, indent=2) + "\n")
    with tempfile.TemporaryDirectory(prefix="chain-", dir=private) as directory:
        chain = Path(directory)
        env = {key: value for key, value in os.environ.items() if not key.startswith("HGS_") and key not in ["TMUX", "TMUX_PANE"]}
        env["ZERUS_TEAMS_ENGINE_PROBE"] = str(chain)
        selector = "teams::ops::tests::base_engine_previous::advance_base_two_engines_previous_holds_store_and_refuses_post_advance_writes"
        with (evidence / "previous-open-store.log").open("w") as output:
            child = subprocess.Popen([str(previous), "--ignored", "--exact", selector, "--nocapture", "--test-threads=1"], cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
            try:
                deadline = time.monotonic() + 60
                while not (chain / "ready.json").exists():
                    if child.poll() is not None:
                        raise RuntimeError("Previous engine exited before publishing its store")
                    if time.monotonic() > deadline:
                        raise TimeoutError("Previous engine did not publish its store")
                    time.sleep(0.02)
                step(current, "teams::ops::tests::base_engines::advance_base_two_engines_new_advances_and_preserves_unpinned_rows", evidence, env)
                (chain / "advanced").touch()
                while not (chain / "refused").exists():
                    if child.poll() is not None:
                        raise RuntimeError("Previous cached writer did not refuse cleanly")
                    if time.monotonic() > deadline:
                        raise TimeoutError("Previous cached writer did not settle")
                    time.sleep(0.02)
                step(current, "teams::ops::tests::base_engines::advance_base_two_engines_new_writer_continues_after_old_writer_refusal", evidence, env)
                (chain / "verified").touch()
                assert child.wait(timeout=10) == 0
            finally:
                (chain / "advanced").touch()
                (chain / "verified").touch()
                try:
                    child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    child.terminate()
                    child.wait(timeout=10)
    print("Both real engines: old reopened and cached writers refused; legacy rows preserved; new writer continues.", flush=True)


if __name__ == "__main__":
    main()
