#!/usr/bin/env python3
"""Check two actual native4-capable source-built Teams test images locally.

This interoperability contract expects both supported writers to succeed.
It does not exercise a missing-function fence or a released pre-base image.
Each build and the paired run is a separate bounded heavy-wrapper step.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
SOURCES = {
    "previous": "5216b5b290a67e59b65665fa19d545bb3eb2ce5d",
    "current": "64eeb5a316f1bca1601b5c732c1d9a2a98d04a52",
}
FIXTURE = ROOT / "tests/fixtures/teams/base_engine_native4.rs"
MODULE = "src/teams/ops/tests/mod.rs"
INJECTION = b"\nmod base_engine_native4;\n"
PREFIX = "teams::ops::tests::base_engine_native4::"
SELECTORS = {
    "previous": PREFIX + "native4_predecessor_cached_and_reopened_writers_remain_compatible",
    "advance": PREFIX + "native4_current_advances_without_rewriting_existing_assignments",
    "continue": PREFIX + "native4_current_continues_after_predecessor_cached_and_reopened_writes",
}


class ContractError(RuntimeError):
    """An unsupported source, image or execution contract; never a fallback."""


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def audit_source(root, commit):
    mod = git(root, "show", commit + ":src/teams/store/mod.rs")
    versions = subprocess.run(
        ["git", "-C", str(root), "show", commit + ":src/teams/store/versions.rs"],
        capture_output=True, check=False,
    ).stdout
    literal = b"const SCHEMA_VERSION: u32 = 4;" in mod
    registered = (b"const SCHEMA_VERSION: u32 = versions::CORE_CURRENT;" in mod
                  and b"const CORE_CURRENT: u32 = CORE_NATIVE_PERSON;" in versions
                  and b"CORE_NATIVE_PERSON = 4;" in versions)
    bases = subprocess.run(
        ["git", "-C", str(root), "show", commit + ":src/teams/store/bases.rs"],
        capture_output=True, check=False,
    ).stdout
    capability = b'"zerus_teams_base_pins_v13"' in bases
    fence = b"execution_fence(db)?;" in mod
    if not ((literal or registered) and capability and fence):
        raise ContractError(f"{commit} is not the audited native4-capable contract; no substitute image is built")
    return {"commit": commit, "tree": git(root, "rev-parse", commit + "^{tree}").decode().strip(),
            "core": 4, "base_pin_capability": capability, "cached_execution_fence": fence,
            "store_source_sha256": hashlib.sha256(mod).hexdigest(),
            "bases_source_sha256": hashlib.sha256(bases).hexdigest()}


def contract():
    return {"contract": "native4-capable-interoperability",
            "sources": {label: audit_source(ROOT, commit) for label, commit in SOURCES.items()},
            "fixture_sha256": digest(FIXTURE)}


def workspace():
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    if target == ROOT or not target.is_relative_to(ROOT):
        raise ContractError("CARGO_TARGET_DIR must be below this worktree")
    return target / "teams-base-native4"


def environment(work):
    original_home = Path.home()
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("HGS_") and key not in ("TMUX", "TMUX_PANE")}
    env.setdefault("RUSTUP_HOME", str(original_home / ".rustup"))
    env.setdefault("CARGO_HOME", str(original_home / ".cargo"))
    for key, name in [("HOME", "home"), ("XDG_STATE_HOME", "state"), ("XDG_CONFIG_HOME", "config"),
                      ("XDG_CACHE_HOME", "cache"), ("TMPDIR", "tmp"), ("TMUX_TMPDIR", "tmux")]:
        path = work / "environment" / name
        path.mkdir(parents=True, exist_ok=True, mode=0o700)
        env[key] = str(path)
    env.update(CARGO_BUILD_JOBS="4", RUST_TEST_THREADS="4", CARGO_INCREMENTAL="0", RAYON_NUM_THREADS="4")
    env.setdefault("CARGO_PROFILE_DEV_DEBUG", "0")
    env.setdefault("CARGO_PROFILE_TEST_DEBUG", "0")
    return env


def prepare(label, work, evidence):
    commit = SOURCES[label]
    audit = audit_source(ROOT, commit)
    area = work / label
    area.mkdir(parents=True, exist_ok=True)
    source = area / "source"
    archive = area / "source.tar"
    with archive.open("wb") as output:
        subprocess.run(["git", "-C", str(ROOT), "archive", commit], stdout=output, check=True)
    archive_sha = digest(archive)
    inventory = {}
    with tarfile.open(archive) as entries:
        if not source.exists():
            source.mkdir()
            entries.extractall(source, filter="data")
        for member in entries.getmembers():
            if member.isfile():
                expected = entries.extractfile(member).read()
                if member.name == MODULE:
                    expected += INJECTION
                    path = source / member.name
                    if path.read_bytes() == expected[:-len(INJECTION)]:
                        path.write_bytes(expected)
                path = source / member.name
                if path.read_bytes() != expected:
                    raise ContractError(f"Unexpected archived-source change: {label}/{member.name}")
                inventory[member.name] = hashlib.sha256(expected).hexdigest()
            elif member.issym():
                if not (source / member.name).is_symlink() or os.readlink(source / member.name) != member.linkname:
                    raise ContractError(f"Unexpected archived-source link: {member.name}")
    test = source / "src/teams/ops/tests/base_engine_native4.rs"
    shutil.copy2(FIXTURE, test)
    inventory[str(test.relative_to(source))] = digest(test)
    archive.unlink()
    audit.update(archive_sha256=archive_sha, fixture_sha256=digest(FIXTURE),
                 source_inventory_sha256=hashlib.sha256(json.dumps(inventory, sort_keys=True).encode()).hexdigest(),
                 injected_test_module=MODULE, injected_module_sha256=digest(source / MODULE))
    (evidence / (label + "-source-seal.json")).write_text(json.dumps(audit, indent=2) + "\n")
    return source, audit


def build(label, work, evidence):
    source, seal = prepare(label, work, evidence)
    env = environment(work)
    env["CARGO_TARGET_DIR"] = str(work / label / "cargo")
    command = ["cargo", "+1.99.1", "test", "--locked", "-j", "4", "--bin", "hgs", "--no-run", "--message-format=json"]
    with (evidence / (label + "-build.jsonl")).open("w") as output, (evidence / (label + "-build.stderr")).open("w") as error:
        result = subprocess.run(command, cwd=source, env=env, stdout=output, stderr=error)
    result.check_returncode()
    outputs = [json.loads(line) for line in (evidence / (label + "-build.jsonl")).read_text().splitlines() if line.startswith("{")]
    images = [row["executable"] for row in outputs if row.get("reason") == "compiler-artifact" and row.get("executable")
              and row.get("profile", {}).get("test") and row["target"]["name"] == "hgs"]
    if len(images) != 1:
        raise ContractError(f"Expected one actual {label} test image, got {images}")
    image = work / label / "engine"
    shutil.copy2(images[0], image)
    seal.update(image=str(image), image_sha256=digest(image), toolchain="1.99.1",
                profile={key: env[key] for key in ["CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG", "CARGO_INCREMENTAL"]})
    (work / (label + "-image.json")).write_text(json.dumps(seal, indent=2) + "\n")
    (evidence / (label + "-image.json")).write_text(json.dumps(seal, indent=2) + "\n")
    print(f"Built {label} source-built test image at {SOURCES[label]}", flush=True)



def lint_current(work, evidence):
    source, _seal = prepare("current", work, evidence)
    env = environment(work)
    env["CARGO_TARGET_DIR"] = str(work / "current" / "cargo")
    command = ["cargo", "+1.99.1", "clippy", "--locked", "-j", "4", "--all-targets", "--", "-D", "warnings"]
    with (evidence / "current-clippy.log").open("w") as output:
        result = subprocess.run(command, cwd=source, env=env, stdout=output, stderr=subprocess.STDOUT)
    result.check_returncode()


def images(work, evidence):
    result = {}
    for label in SOURCES:
        path = work / (label + "-image.json")
        if not path.exists():
            raise ContractError("Build both native4 images before run; no fallback build is performed")
        seal = json.loads(path.read_text())
        if (seal["commit"] != SOURCES[label] or seal["fixture_sha256"] != digest(FIXTURE)
                or digest(Path(seal["image"])) != seal["image_sha256"]):
            raise ContractError(f"Stale or changed {label} image: rebuild the sealed fixture")
        result[label] = seal
    if result["previous"]["image_sha256"] == result["current"]["image_sha256"]:
        raise ContractError("The two source-built test images collided")
    (evidence / "images.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def command(image, selector):
    return [image, "--ignored", "--exact", selector, "--nocapture", "--test-threads=1"]


def passed(log):
    if "test result: ok. 1 passed;" not in log.read_text():
        raise ContractError(f"The named image control did not execute exactly once: {log.name}")


def until(path, child):
    deadline = time.monotonic() + 90
    while not path.exists():
        if child.poll() is not None:
            raise ContractError(f"Predecessor exited before {path.name}; inspect predecessor-run.log")
        if time.monotonic() > deadline:
            raise ContractError(f"Timed out waiting for {path.name}")
        time.sleep(0.02)


def run(work, evidence):
    pair = images(work, evidence)
    env = environment(work)
    with tempfile.TemporaryDirectory(prefix="native4-", dir=work) as directory:
        chain = Path(directory)
        env["ZERUS_TEAMS_NATIVE4_PROBE"] = str(chain)
        log = evidence / "predecessor-run.log"
        with log.open("w") as output:
            child = subprocess.Popen(command(pair["previous"]["image"], SELECTORS["previous"]), cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
            try:
                until(chain / "ready.json", child)
                for step, marker in [("advance", "advanced"), ("continue", "verified")]:
                    if step == "continue":
                        until(chain / "predecessor.json", child)
                    step_log = evidence / ("current-" + step + ".log")
                    with step_log.open("w") as sink:
                        completed = subprocess.run(command(pair["current"]["image"], SELECTORS[step]), cwd=ROOT, env=env,
                                                   stdout=sink, stderr=subprocess.STDOUT, timeout=90)
                    completed.check_returncode()
                    passed(step_log)
                    (chain / marker).touch()
                if child.wait(timeout=90) != 0:
                    raise ContractError("Predecessor failed; inspect predecessor-run.log")
                passed(log)
                facts = {name: json.loads((chain / (name + ".json")).read_text())
                         for name in ["ready", "predecessor", "current"]}
                (evidence / "product-facts.json").write_text(json.dumps(facts, indent=2) + "\n")
            finally:
                # Terminate only our own child on failure. Never fabricate a
                # successful peer marker to release a failed scenario.
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=10)
    (evidence / "execution-counts.json").write_text(json.dumps({"previous": 1, "current": 2, "failed": 0,
        "contract": "native4-capable-interoperability", "missing_function_sql_refusal_proved": False}, indent=2) + "\n")
    print("Native4 interoperability: predecessor 1/current 2 controls passed; SQL missing-function refusal is a separate fault-seam check.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inspect", action="store_true", help="audit pinned real Git sources without preparing/building images")
    parser.add_argument("--step", choices=["all", "build-previous", "build-current", "run", "lint-current"], default="all")
    parser.add_argument("--evidence", type=Path, default=ROOT / "artifacts/test-results/teams-base-compatibility")
    parser.add_argument("--heavy-wrapper", type=Path, help="shared machine wrapper, acquired separately for each bounded step")
    args = parser.parse_args()
    audit = contract()
    if args.inspect:
        print(json.dumps(audit, indent=2))
        return
    work = workspace()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    (evidence / "contract.json").write_text(json.dumps(audit, indent=2) + "\n")
    if args.step == "all" or args.heavy_wrapper:
        steps = ["build-previous", "build-current", "run"] if args.step == "all" else [args.step]
        for step in steps:
            cmd = [sys.executable, str(Path(__file__).resolve()), "--step", step, "--evidence", str(evidence)]
            if args.heavy_wrapper:
                cmd.insert(0, str(args.heavy_wrapper.resolve()))
            print(f"Starting bounded native4 step: {step}", flush=True)
            subprocess.run(cmd, cwd=ROOT, check=True)
    elif args.step == "run":
        run(work, evidence)
    elif args.step == "lint-current":
        lint_current(work, evidence)
    else:
        build(args.step.removeprefix("build-"), work, evidence)


if __name__ == "__main__":
    try:
        main()
    except (ContractError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        print(f"Native4 compatibility refused: {error}", file=sys.stderr)
        sys.exit(1)
