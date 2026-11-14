#!/usr/bin/env python3
"""Opt-in hosted NTFS qualification of the conservative Windows refusal path.

Each image runs as a fresh ordinary local user. Mutants are build-local only;
the const-false gate and all source bytes are restored before the final control.
This demonstrates Unknown handling, not qualified namespace durability.
"""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "artifacts/test-results/start-refusal"
GATE = ROOT / "crates/desktop-read-policy/src/task_controls.rs"
ENV = {k: v for k, v in os.environ.items() if not k.startswith(("HGS_", "DSH_"))}
ENV.update(CARGO_INCREMENTAL="0", CARGO_BUILD_JOBS="4", CARGO_TARGET_DIR=str(ROOT / "target"))


def run(args, log, timeout):
    with log.open("w", encoding="utf-8") as output:
        return subprocess.run(args, cwd=ROOT, env=ENV, stdin=subprocess.DEVNULL,
                              stdout=output, stderr=subprocess.STDOUT,
                              timeout=timeout, check=False).returncode


def build(label):
    log = OUT / (label + "-build.jsonl")
    status = run(["cargo", "+1.99.1", "test", "--locked", "--no-run", "--bin", "hgs",
                  "-j", "4", "--message-format=json"], log, 1800)
    if status:
        raise RuntimeError(f"{label}: build failed ({status}); not a killed mutant")
    images = set()
    with log.open(encoding="utf-8") as lines:
        for line in lines:
            try:
                item = json.loads(line)
            except ValueError:
                continue
            if (item.get("reason") == "compiler-artifact" and
                    item.get("target", {}).get("name") == "hgs" and
                    item.get("profile", {}).get("test") and item.get("executable")):
                images.add(item["executable"])
    if len(images) != 1:
        raise RuntimeError(f"{label}: no unique original unit image")
    return Path(images.pop())


def native(label, needle=None):
    image = build(label)
    status = run(["powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-File",
                  str(ROOT / "scripts/ci/windows-start-refusal-user.ps1"),
                  "-TestExe", str(image), "-Label", label], OUT / (label + ".log"), 420)
    evidence = OUT / label
    report = json.loads((evidence / "standard-user.json").read_text(encoding="utf-8"))
    cleanup = json.loads((evidence / "cleanup.json").read_text(encoding="utf-8"))
    launch = json.loads((evidence / "launch.json").read_text(encoding="utf-8"))
    if (report["administrator_sid_present"] or report["filesystem"] != "NTFS" or
            not report["sid"].startswith("S-1-5-21-") or not report["settled"] or
            not all(call["physical_wait"] for call in report["calls"]) or
            not all(cleanup.values()) or not launch["physical_wait"] or
            launch["load_user_profile"] is not False or
            launch["image_sha256"] != hashlib.sha256(image.read_bytes()).hexdigest()):
        raise RuntimeError(f"{label}: incomplete native custody/standard-user evidence")
    if needle is None:
        if status or not report["complete"] or len(report["calls"]) != 5:
            raise RuntimeError(f"{label}: required native controls failed")
    else:
        failed = [call for call in report["calls"] if call["exit"] != 0]
        text = "\n".join(call["stdout"] + call["stderr"] for call in failed)
        if not status or report["complete"] or not failed or needle not in text or "panicked at" not in text:
            raise RuntimeError(f"{label}: no expected runtime assertion; not a killed mutant")
    return {"label": label, "result": "killed" if needle else "passed",
            "image_sha256": launch["image_sha256"], "filesystem": report["filesystem"],
            "sid": report["sid"], "filters": len(report["calls"])}


def main():
    if (sys.platform != "win32" or ENV.get("GITHUB_ACTIONS") != "true" or
            ENV.get("RUNNER_ENVIRONMENT") != "github-hosted" or
            ENV.get("GITHUB_EVENT_NAME") != "workflow_dispatch"):
        raise RuntimeError("Requires an explicit disposable hosted Windows run")
    OUT.mkdir(parents=True, exist_ok=True)
    gate = GATE.read_bytes()
    if b"pub const TASK_CONTROLS: bool = false;" not in gate:
        raise RuntimeError("Task-control activation is outside this qualification")
    # These remove real barriers independently. A read-only file flush can fail
    # before the directory check, so the lookup mutant removes BOTH completions.
    mutants = [
        ("namespace-noop", "src/state/repositories/directory.rs",
         'Err("Native namespace durability is unverified on this computer".into())',
         'Ok(())', "the original namespace completion remains unqualified"),
        ("lookup-without-completion", "src/state/input/task_refusal.rs",
         '    file.sync_all().map_err(|e| e.to_string())?;\n    dir.sync()?;',
         '', "read-back must not mint a durable refusal proof"),
        ("replace-existing", "src/state/repositories/directory/windows_publication.rs",
         '(*info).Anonymous.ReplaceIfExists = false;',
         '(*info).Anonymous.ReplaceIfExists = true;',
         'failure_after_publication_keeps_the_original_uuid_unknown_and_never_replaces'),
        ("uncertain-as-refused", "src/tasks/dispatch.rs",
         '_ => reply("uncertain", UNCERTAIN),',
         '_ => reply("refused", UNCERTAIN),',
         'unavailable_publication_never_becomes_a_definite_refusal_or_delivery'),
        ("restart-without-a-control", "src/state/input/task_refusal/windows_tests.rs",
         'const RESTART: &str = "state::input::task_refusal::windows_tests::retained_refusal_restart_probe";',
         'const RESTART: &str = "not_a_control";',
         'restart probe must execute exactly one control'),
    ]
    source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if source != ENV.get("QUALIFICATION_SOURCE"):
        raise RuntimeError("Qualification checkout differs from the admitted source")
    originals = {ROOT / name: (ROOT / name).read_bytes() for _, name, *_ in mutants}
    result = {"source": source, "namespace_durability": "unqualified",
              "task_controls": False, "runs": []}
    try:
        result["runs"].append(native("candidate"))
        for label, name, before, after, needle in mutants:
            path = ROOT / name
            original = originals[path]
            if original.count(before.encode()) != 1:
                raise RuntimeError(f"{label}: source no longer matches the reviewed mutant")
            try:
                path.write_bytes(original.replace(before.encode(), after.encode()))
                if GATE.read_bytes() != gate:
                    raise RuntimeError("Task-control gate changed")
                result["runs"].append(native(label, needle))
            finally:
                path.write_bytes(original)
        result["runs"].append(native("restored"))
        if GATE.read_bytes() != gate or any(path.read_bytes() != content for path, content in originals.items()):
            raise RuntimeError("Original source was not restored")
        result["complete"] = True
    finally:
        for path, content in originals.items():
            path.write_bytes(content)
        (OUT / "qualification.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
