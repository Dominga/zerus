"""Keep client-core parallel, source-bound and mandatory after leaving Quick."""
import contextlib
import io
from itertools import product
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import test_ci_security as security
import test_ci_local as local
import test_ci_disk as disk

ROOT = Path(__file__).resolve().parents[1]
COMMAND = "bash scripts/ci/client-core.sh"


class ParallelClientCore(unittest.TestCase):
    def workflow(self):
        return (ROOT / ".github/workflows/checks.yml").read_text()

    def test_client_core_is_an_unconditional_source_bound_parallel_job(self):
        checks = self.workflow()
        core = security.job(checks, "client-core")
        self.assertEqual(security.field(core, "needs"), "source")
        self.assertEqual(security.field(core, "runs-on"), "ubuntu-24.04")
        self.assertGreater(int(security.field(core, "timeout-minutes")), 0)
        self.assertLessEqual(int(security.field(core, "timeout-minutes")), 30)
        self.assertNotRegex(core, r"(?m)^    if:")
        self.assertIn("ref: ${{ needs.source.outputs.commit }}", core)
        self.assertIn("persist-credentials: false", core)
        install = "rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy"
        self.assertIn(install, core)
        self.assertLess(core.index(install), core.index(COMMAND))
        self.assertNotIn("continue-on-error", core)
        for variable in ("CARGO_INCREMENTAL", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG"):
            self.assertIn(variable + ': "0"', core)
        run = next(step for step in security.steps(core) if "run: " + COMMAND in step)
        self.assertNotIn("if:", run)
        self.assertNotIn("continue-on-error", run)

    def test_the_existing_client_core_sequence_runs_once_outside_quick(self):
        checks = self.workflow()
        self.assertNotIn(COMMAND, security.job(checks, "quick"))
        self.assertEqual(checks.count("run: " + COMMAND), 1)
        script = (ROOT / "scripts/ci/client-core.sh").read_text()
        self.assertIn("set -euo pipefail", script)
        self.assertEqual([line for line in script.splitlines() if line.startswith("cargo ")], [
            "cargo +1.99.1 fmt --check --manifest-path client/core/Cargo.toml",
            "cargo +1.99.1 clippy --locked --manifest-path client/core/Cargo.toml --target-dir target --all-targets -- -D warnings",
            "cargo +1.99.1 test --locked --manifest-path client/core/Cargo.toml --target-dir target",
        ])

    def test_client_core_is_required_in_every_selected_platform_combination(self):
        gate = security.job(self.workflow(), "gate")
        names = security.field(gate, "needs").strip("[]").split(", ")
        code = compile(security.inline_python(gate), "checks-client-core-gate", "exec")
        platforms = {
            "RUN_LINUX": {"cli", "client-native-linux"},
            "RUN_ARCH": {"arch-package"},
            "RUN_MACOS": {"cli-macos-debug", "cli-macos-core", "cli-macos-native", "cli-macos",
                          "clippy-macos", "client-native-macos-core", "client-native-macos-tauri", "connector-macos"},
            "RUN_WINDOWS": {"windows-tests", "windows-desktop", "windows-host", "client-native-windows"},
            "RUN_IOS": {"ios"},
        }
        for choices in product((False, True), repeat=len(platforms)):
            selected = dict(zip(platforms, choices))
            base = {name: {"result": "success"} for name in names}
            for flag, enabled in selected.items():
                if not enabled:
                    for name in platforms[flag]:
                        base[name] = {"result": "skipped"}
            for result in ("success", "failure", "cancelled", "skipped"):
                statuses = dict(base, **{"client-core": {"result": result}})
                environment = {key: str(value).lower() for key, value in selected.items()}
                environment["RESULTS"] = json.dumps(statuses)
                with self.subTest(platforms=selected, client_core=result), patch.dict(os.environ, environment), \
                        contextlib.redirect_stdout(io.StringIO()):
                    if result == "success":
                        exec(code, {})
                    else:
                        with self.assertRaises(SystemExit):
                            exec(code, {})
        self.assertIn("client-core", names)

    def test_client_core_reuses_a_read_only_cache_without_new_write_authority(self):
        core = security.job(self.workflow(), "client-core")
        self.assertIn("uses: actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", core)
        self.assertIn("hashFiles('client/core/Cargo.lock')", core)
        self.assertIn("steps.rust.outputs.rustc", core)
        self.assertIn("steps.rust.outputs.epoch", core)
        self.assertNotIn("actions/cache/save@", core)
        self.assertEqual(security.field(core, "cache-mode"), "read")
        self.assertNotIn("cache-mode: write", core)
        self.assertNotIn("secrets", core)
        self.assertNotIn("permissions:", core)

    def test_local_runner_keeps_the_same_script_after_the_job_split(self):
        workflow = {"jobs": {"client-core": {"runs-on": "ubuntu-24.04", "steps": [
            {"name": "Desktop client core crate (format, lints, tests)", "run": COMMAND}]}}}
        job, = local.CI.plan(workflow, "")
        self.assertEqual(len(job["steps"]), 1)
        self.assertEqual(job["steps"][0]["body"], COMMAND)

    def test_disposable_hosted_disk_preparation_admits_the_new_job_only(self):
        with tempfile.TemporaryDirectory(prefix="ci-core-") as folder:
            root = Path(folder)
            environment = dict(GITHUB_ACTIONS="true", RUNNER_ENVIRONMENT="github-hosted", RUNNER_OS="Linux",
                               GITHUB_JOB="client-core", GITHUB_WORKSPACE=str(root))
            with patch.dict(os.environ, environment), patch.object(disk.DISK.Path, "cwd", return_value=root):
                self.assertEqual(disk.DISK.hosted_workspace(), root.resolve())
                self.assertIn("client-core", disk.DISK.CLEANUP_JOBS)
                with patch.dict(os.environ, RUNNER_ENVIRONMENT="self-hosted"), self.assertRaises(ValueError):
                    disk.DISK.hosted_workspace()

    def test_each_existing_client_core_command_still_fails_the_sequence(self):
        with tempfile.TemporaryDirectory(prefix="ci-core-") as folder:
            root = Path(folder)
            cargo = root / "cargo"
            cargo.write_text('#!/bin/sh\ncase "$*" in *"$FAIL_ON"*) exit 19 ;; esac\nexit 0\n')
            cargo.chmod(0o700)
            for command in ("fmt --check", "clippy --locked", "test --locked"):
                with self.subTest(command=command):
                    result = subprocess.run(["bash", "scripts/ci/client-core.sh"], cwd=ROOT,
                        env=dict(PATH=str(root) + ":/usr/bin:/bin", HOME=str(root), FAIL_ON=command),
                        capture_output=True, text=True, timeout=10)
                    self.assertEqual(result.returncode, 19, result.stdout + result.stderr)
