#!/usr/bin/env python3
"""Compile the complete hgs test binary with production call-site mutations.

No extracted helpers or policy shims. Fixtures run the actual Native::deliver,
original-child runner and SQLite stores, with an isolated stub executable.
Use --case NAME to run one regression mutant after an implementation change.
"""
import argparse
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[4]
OUT = ROOT / "artifacts/test-results/wp17-outcome"
BASE = "src/desktop/teams/confirm/"
DELIVERY = "desktop::teams::confirm::native::original::delivery::"
LEDGER = "desktop::teams::confirm::ledger::"
OWNER = "desktop::teams::confirm::native::ownership_launch::"


def command(source, binary):
    deps, fp = ROOT / "target/debug/deps", ROOT / "target/debug/.fingerprint"
    targets = [p for p in fp.glob("hgs-*/test-bin-hgs.json") if (deps / p.parent.name).is_file()]
    target = max(targets, key=os.path.getmtime)
    wanted = {dep[1]: dep[3] for dep in json.loads(target.read_text())["deps"]}
    cmd = ["rustc", "+1.99.1", "--edition=2021", "--test", "--crate-name", "hgs", str(source / "src/main.rs"),
           "-o", str(binary), "-L", "dependency=" + str(deps), "-C", "debuginfo=0", "-C", "codegen-units=8"]
    for name, identity in wanted.items():
        if name == "build_script_build":
            continue
        candidates = [p for p in fp.glob("*/lib-" + name)
                      if int.from_bytes(bytes.fromhex(p.read_text()), "little") == identity]
        if len(candidates) != 1:
            raise RuntimeError("Cannot resolve Cargo dependency: " + name)
        suffix = candidates[0].parent.name.rsplit("-", 1)[1]
        cmd += ["--extern", name + "=" + str(deps / ("lib" + name + "-" + suffix + ".rlib"))]
    return cmd


def cases():
    yield "terminal_unknown_race", BASE + "ledger.rs", LEDGER + "concurrency::", "race"
    yield "launch_object_evidence", BASE + "native/ownership.rs", OWNER, ("self.anchored(b)? && ", "")
    yield "card_read_bound", BASE + "ledger.rs", LEDGER + "cards::oversized_card_read", ("length(CAST(request AS BLOB))<=?2", "length(CAST(request AS BLOB))>=0*?2")
    yield "card_write_bound", BASE + "ledger.rs", LEDGER + "cards::card_write", "write_bound"
    original = BASE + "native/original.rs"
    yield "final_admission_call", original, DELIVERY, "call"
    yield "final_stored_facts", original, DELIVERY + "current_team_member", ("super::super::policy::stored(b, &facts)?;", "();")
    yield "final_delegation", original, DELIVERY + "delegation_revoked", ("!delegated", "false")
    yield "final_resume_delegation", original, DELIVERY + "delegation_revoked", ("facts.team.settings.delegations.resume_exited", "true")
    yield "final_directory_anchor", original, DELIVERY + "final_store_ownership", ("!self.anchored(b)?", "false")
    yield "final_root", original, DELIVERY + "root_replacement", ("!roots.root_current()", "false")
    yield "final_pins_present", original, DELIVERY + "missing_pins", ("self.pins.is_empty()", "false")
    yield "final_pins_current", original, DELIVERY + "final_store_ownership", ("!self.pins.iter().all(super::ownership::Pin::current)", "false")
    yield "final_machine", original, DELIVERY + "final_store_ownership", ("crate::state::lifecycle_receipt_machine(self.config)? != self.machine", "false")
    yield "final_record_read", original, DELIVERY + "final_native_record", ("held_current(roots, snapshot)?", "raw.clone()")
    yield "final_record_identity", original, DELIVERY + "final_native_record", ("unchanged_record(&current, raw)?;", "();")
    yield "final_pause", original, DELIVERY + "final_native_record", ("super::observation::paused(&current)", "false")
    yield "final_late_stop", original, DELIVERY + "a_stop_after", ("self.stop.load(Ordering::Acquire)", "false")
    yield "final_self_exit", original, DELIVERY + "missing_pins", ("!super::observation::self_exit(&current, snapshot.archive.is_some())", "false")
    yield "producer_records_object", "src/desktop/teams/launch/effects.rs", "desktop::teams::launch::effects::checkout_tests::", "producer"

    admission_test = DELIVERY + "admission::"
    yield "admission_facts_outside_lock", original, admission_test, "outside_lock"
    yield "admission_deferred_transaction", original, admission_test, ("TransactionBehavior::Immediate", "TransactionBehavior::Deferred")
    yield "admission_released_before_spawn", "src/desktop/original_child.rs", "desktop::original_child::admission_tests::", ("let spawned = command.spawn();\n                drop(admission);", "drop(admission);\n                let spawned = command.spawn();")
    yield "admission_held_until_child_end", "src/desktop/original_child.rs", "desktop::original_child::admission_tests::", "hold_guard"
    yield "admission_discarded_by_caller", original, admission_test, ("                Ok(admission)", "                drop(admission); Ok(())")
    yield "admission_bound_computer", original, admission_test + "the_store_and_native_capture", ("b.machine != self.machine", "false")
    yield "admission_busy_is_uncertain", original, admission_test + "a_busy", ("Ended::NotStarted if waiting.get() => Outcome::Retry", "Ended::NotStarted if waiting.get() => Outcome::Unknown(\"busy\".into())")
    yield "waiting_unknown_reclaimed", BASE + "ledger.rs", LEDGER + "tests::a_recorded_nonstart", ("row.state == \"waiting\" && row.plan == *plan", "matches!(row.state.as_str(), \"waiting\" | \"unknown\") && row.plan == *plan")
    yield "waiting_quota_ignored", BASE + "ledger.rs", LEDGER + "tests::a_nonstart_reclaim", ("fresh && plan.kind == Kind::Resume && quota(tx, &plan.binding, now, &plan.key)?", "false")
    yield "waiting_opens_card", BASE + "engine.rs", "desktop::teams::confirm::engine::tests::busy_admission", ("if row.state == \"waiting\" {\n        return Ok(());", "if false {\n        return Ok(());")
    yield "startup_conversation_null", BASE + "policy.rs", "desktop::teams::confirm::policy::tests::a_real_startup_card", ("        Value::Null => Some(\"\"),", "        Value::Null => None,")
    yield "retry_outcome_detail", BASE + "engine.rs", DELIVERY + "retry::wp17_own_successful_retry_must_not_journal_a_nonstart", ('            row.detail = "The exact native receipt confirmed the original request".into();', "")


def mutate(text, edit):
    if isinstance(edit, tuple):
        old, new = edit
        if old not in text:
            raise RuntimeError("Mutation target missing: " + old)
        return text.replace(old, new, 1)
    if edit == "race":
        begin = text.index("        let tx =", text.index("    pub fn save("))
        end = text.index("        let expected =", begin)
        current = text[begin:end]
        row = current.index("        let mut old =")
        return text[:begin] + current[row:].replace("record(&tx,", "record(&self.0,") + current[:row] + text[end:]
    if edit == "write_bound":
        begin = text.index("    pub fn save_card(")
        part = text[begin:]
        if "if raw.len() > ROW_LIMIT" not in part:
            raise RuntimeError("Writer limit target missing")
        return text[:begin] + part.replace("if raw.len() > ROW_LIMIT", "if false", 1)
    if edit == "call":
        old = '                self.final_admit(row, &roots, &snapshot, &raw, &admission)\n                    .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;'
        return mutate(text, (old, ""))
    if edit == "outside_lock":
        begin = text.index("                let admission = admission(db)")
        mid = text.index("                self.final_admit(", begin)
        end = text.index("                Ok(admission)", mid)
        locked = text[begin:mid]
        read = text[mid:end].replace("&admission)", "db)")
        return text[:begin] + read + locked + text[end:]
    if edit == "hold_guard":
        text = mutate(text, ("    let spawned = {\n", "    let mut held_admission = None;\n    let spawned = {\n"))
        return mutate(text, ("                drop(admission);", "                held_admission = Some(admission);"))
    if edit == "producer":
        return mutate_producer(text)
    raise RuntimeError("Unknown mutation")


def mutate_producer(text):
    # Remove both real producer calls, keeping every helper and test unchanged.
    lines = text.splitlines(True)
    removed = 0
    for end in reversed(range(len(lines))):
        if ".record_checkout(plan, &tree)?;" in lines[end]:
            start = end
            while "#[cfg(" not in lines[start]:
                start -= 1
            del lines[start:end + 1]
            removed += 1
    if removed != 2:
        raise RuntimeError("Expected both verified producer paths")
    return "".join(lines)


def run(cmd, log, env):
    log.write("COMMAND: " + " ".join(cmd) + "\n")
    log.flush()
    offset = log.tell()
    start = time.monotonic()
    result = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT, env=env, timeout=300)
    log.write(f"EXIT: {result.returncode}; SECONDS: {time.monotonic()-start:.3f}\n")
    log.flush()
    if cmd[0] != "rustc":
        with open(log.name, "rb") as saved:
            saved.seek(offset)
            output = saved.read().decode("utf-8", "replace")
        count = re.search(r"running (\d+) tests?", output)
        if not count or int(count[1]) == 0 or result.returncode not in (0, 101):
            raise RuntimeError("The production test did not execute normally")
    return result.returncode


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--case")
    parser.add_argument("--spawn", action="store_true", help="the spawn/admission and definite-nonstart regressions")
    args = parser.parse_args()
    selected = [case for case in cases() if (not args.case or case[0] == args.case)
                and (not args.spawn or case[0].startswith(("admission_", "waiting_")))]
    if not selected:
        parser.error("Unknown case")
    OUT.mkdir(parents=True, exist_ok=True)
    name = "production-mutants" + ("-" + args.case if args.case else "-spawn" if args.spawn else "") + ".log"
    with tempfile.TemporaryDirectory(prefix="zerus-confirm-build-", dir=ROOT / "target") as build, \
            tempfile.TemporaryDirectory(prefix="zerus-confirm-production-") as temporary, (OUT / name).open("w") as log:
        source = Path(temporary)
        shutil.copytree(ROOT / "src", source / "src")
        for entry in ROOT.iterdir():
            if entry.name not in ["src", "target", ".git"]:
                (source / entry.name).symlink_to(entry, target_is_directory=entry.is_dir())
        binary = Path(build) / "full-tests"
        cmd = command(source, binary)
        env = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT), CARGO_PKG_VERSION="1.58.0")
        if run(cmd, log, env) or run([str(binary), "desktop::teams::", "--test-threads=8"], log, env):
            raise RuntimeError("Production baseline failed")
        killed = 0
        for name, file, test, edit in selected:
            path = source / file
            original = path.read_text()
            path.write_text(mutate(original, edit))
            log.write("MUTANT: " + name + "\n")
            if run(cmd, log, env):
                raise RuntimeError("Mutant did not compile: " + name)
            result = run([str(binary), test, "--test-threads=1"], log, env)
            if result:
                killed += 1
            print(("KILLED " if result else "SURVIVED ") + name, flush=True)
            path.write_text(original)
        summary = f"PRODUCTION MUTANTS: {len(selected)} executed; {killed} killed; {len(selected)-killed} survived"
        print(summary, flush=True)
        log.write(summary + "\n")
        if killed != len(selected):
            raise SystemExit(1)


if __name__ == "__main__":
    main()
