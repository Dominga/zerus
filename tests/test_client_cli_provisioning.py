#!/usr/bin/env python3
"""Maintained callers hand the Tauri drive-folder tests the CLI built from this source.

Private stand-ins replace Cargo, rustup and the display/packaging helpers; they
only record the actual commands, environments and order (zerus-h43u3). The
real drive-folder integration tests still need a real source-built CLI.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

RECORDER = r'''#!PYTHON
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
record = {'name': name, 'args': args, 'cwd': os.getcwd(),
          'HGS_TEST_BIN': os.environ.get('HGS_TEST_BIN'), 'CARGO_TARGET_DIR': os.environ.get('CARGO_TARGET_DIR')}
cli = os.environ.get('HGS_TEST_BIN')
record['cli_ok'] = bool(cli) and os.path.isabs(cli) and os.path.isfile(cli) and os.access(cli, os.X_OK)
with open(os.environ['CI_FIXTURE_LOG'], 'a') as stream:
    stream.write(json.dumps(record) + '\n')
if name == 'cargo':
    rest = [a for a in args if not a.startswith('+')]
    if rest[:1] == ['build']:
        if os.environ.get('CI_FIXTURE_BUILD') == 'fail':
            sys.exit(7)
        manifest = '--manifest-path' in rest
        if os.environ.get('CI_FIXTURE_BUILD') != 'no-output' and not manifest:
            profile = 'release' if '--release' in rest else 'debug'
            target = os.environ.get('CARGO_TARGET_DIR') or 'target'
            out = pathlib.Path(target, profile, 'hgs')
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text('#!/bin/sh\n')
            out.chmod(0o700)
elif name in ('xvfb-run', 'dbus-run-session'):
    # Run the wrapped command, as the real helpers do.
    while args and args[0].startswith('-'):
        option = args.pop(0)
        if option == '-s':
            args.pop(0)
        if option == '--':
            break
    os.execvp(args[0], args)
'''


class Fixture:
    def __init__(self, test, scripts, extra_tools=()):
        self.directory = tempfile.TemporaryDirectory(prefix="zerus cli provisioning ")
        test.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.workspace = self.root / "work space"
        for script in scripts:
            target = self.workspace / script
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / script, target)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        recorder = RECORDER.replace("#!PYTHON", "#!" + sys.executable)
        for tool in ("cargo", "rustup", "xvfb-run", "dbus-run-session", *extra_tools):
            stub = self.bin / tool
            stub.write_text(recorder)
            stub.chmod(0o700)
        self.log = self.root / "calls.jsonl"
        self.stale = self.root / "stale" / "hgs"
        self.stale.parent.mkdir()
        self.stale.write_text("#!/bin/sh\nexit 99\n")
        self.stale.chmod(0o700)

    def run(self, argv, env=None, cwd=None):
        environment = {k: v for k, v in os.environ.items()
                       if k not in ("CARGO_TARGET_DIR", "GITHUB_ACTIONS", "RUNNER_ENVIRONMENT", "GITHUB_JOB")}
        # A stale inherited candidate must never reach the Tauri tests.
        environment.update(PATH=str(self.bin) + os.pathsep + environment.get("PATH", ""),
                           CI_FIXTURE_LOG=str(self.log), HGS_TEST_BIN=str(self.stale))
        environment.update(env or {})
        if self.log.exists():
            self.log.unlink()
        result = subprocess.run(argv, cwd=cwd or self.workspace, env=environment,
                                capture_output=True, text=True, timeout=60)
        calls = [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        return result, calls


def cargo(calls):
    return [call for call in calls if call["name"] == "cargo"]


def is_cli_build(call):
    return call["args"][1:2] == ["build"] and "--bin" in call["args"] and "hgs" in call["args"]


def is_tauri_test(call):
    args = call["args"]
    return "test" in args and "client/src-tauri/Cargo.toml" in args


class ClientNativeCandidateCli(unittest.TestCase):
    SCRIPT = "scripts/ci/client-native.sh"

    def fixture(self):
        fixture = Fixture(self, [self.SCRIPT])
        # A closed stand-in: the Windows core stage only records its hosted adversary.
        adversary = fixture.workspace / "scripts/ci/windows-frontend-adversary.sh"
        adversary.write_text("exit 0\n")
        return fixture

    def run_stage(self, fixture, platform, stage=None, env=None):
        argv = ["bash", str(fixture.workspace / self.SCRIPT), platform] + ([stage] if stage else [])
        return fixture.run(argv, env)

    def assert_fresh_cli_before_tauri_tests(self, calls, target):
        cargo_calls = cargo(calls)
        builds = [i for i, call in enumerate(cargo_calls) if is_cli_build(call)]
        tests = [i for i, call in enumerate(cargo_calls) if is_tauri_test(call)]
        self.assertEqual(len(builds), 1, cargo_calls)
        self.assertEqual(len(tests), 1, cargo_calls)
        build, test = cargo_calls[builds[0]], cargo_calls[tests[0]]
        # Built from this source with the pinned toolchain into the same target directory.
        self.assertEqual(build["args"], ["+1.99.1", "build", "--locked", "--bin", "hgs"])
        self.assertEqual(Path(build["cwd"]).resolve(), self.workspace.resolve())
        self.assertEqual(tests[0], builds[0] + 1, "the CLI build directly precedes the Tauri tests")
        expected = (target / "debug" / "hgs").resolve()
        self.assertEqual(Path(test["HGS_TEST_BIN"]).resolve(), expected)
        self.assertTrue(os.path.isabs(test["HGS_TEST_BIN"]))
        self.assertTrue(test["cli_ok"])
        self.assertEqual(test["CARGO_TARGET_DIR"], build["CARGO_TARGET_DIR"])
        for call in cargo_calls[:builds[0] + 1]:
            self.assertIsNone(call["HGS_TEST_BIN"], "no earlier phase sees an inherited CLI")
        return build, test

    def test_linux_all_builds_the_candidate_cli_for_its_inline_tauri_tests(self):
        fixture = self.fixture()
        self.workspace = fixture.workspace
        result, calls = self.run_stage(fixture, "linux")
        self.assertEqual(result.returncode, 0, result.stderr)
        _, test = self.assert_fresh_cli_before_tauri_tests(calls, fixture.workspace / "target/client-native")
        self.assertEqual(test["args"], ["+1.99.1", "test", "--locked", "--manifest-path", "client/src-tauri/Cargo.toml"])
        names = [call["name"] for call in calls]
        # The tests still run under the existing headless display wrapper.
        self.assertEqual(names[names.index("xvfb-run") + 1], "cargo")
        self.assertTrue(is_tauri_test(calls[names.index("xvfb-run") + 1]))
        media = [i for i, call in enumerate(cargo(calls)) if "client/media/Cargo.toml" in call["args"]]
        build = [i for i, call in enumerate(cargo(calls)) if is_cli_build(call)][0]
        self.assertLess(media[0], build)

    def test_macos_all_and_tauri_test_build_the_candidate_cli_first(self):
        for stage in ("all", "tauri-test"):
            with self.subTest(stage=stage):
                fixture = self.fixture()
                self.workspace = fixture.workspace
                result, calls = self.run_stage(fixture, "macos", stage)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assert_fresh_cli_before_tauri_tests(calls, fixture.workspace / "target/client-native")
                if stage == "tauri-test":
                    self.assertEqual(len(cargo(calls)), 2)

    def test_phases_without_tauri_tests_build_no_cli_and_pass_none(self):
        for platform, stage in (("macos", "core"), ("macos", "media"), ("macos", "tauri-clippy"),
                                ("windows", "core"), ("windows", "media"), ("windows", "tauri-clippy"),
                                ("windows", "all")):
            with self.subTest(platform=platform, stage=stage):
                fixture = self.fixture()
                result, calls = self.run_stage(fixture, platform, stage)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(cargo(calls))
                self.assertFalse([call for call in cargo(calls) if call["args"][1:2] == ["build"]])
                self.assertFalse([call for call in calls if is_tauri_test(call)])
                self.assertTrue(all(call["HGS_TEST_BIN"] is None for call in calls), calls)

    def test_custom_target_directories_with_spaces_absolute_and_relative(self):
        for kind in ("absolute", "relative"):
            with self.subTest(kind=kind):
                fixture = self.fixture()
                self.workspace = fixture.workspace
                if kind == "absolute":
                    target, value = fixture.root / "custom target" / "t", str(fixture.root / "custom target" / "t")
                else:
                    target, value = fixture.workspace / "rel dir" / "t", "rel dir/t"
                for platform, stage in (("linux", None), ("macos", "tauri-test")):
                    result, calls = self.run_stage(fixture, platform, stage, {"CARGO_TARGET_DIR": value})
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assert_fresh_cli_before_tauri_tests(calls, target)

    def test_a_failed_or_empty_cli_build_stops_before_any_tauri_test(self):
        for mode in ("fail", "no-output"):
            for platform, stage in (("linux", None), ("macos", "all"), ("macos", "tauri-test")):
                with self.subTest(mode=mode, platform=platform, stage=stage):
                    fixture = self.fixture()
                    result, calls = self.run_stage(fixture, platform, stage, {"CI_FIXTURE_BUILD": mode})
                    self.assertNotEqual(result.returncode, 0)
                    self.assertTrue([call for call in cargo(calls) if is_cli_build(call)])
                    self.assertFalse([call for call in calls if is_tauri_test(call)])
                    self.assertNotIn("xvfb-run", [call["name"] for call in calls])
                    if mode == "no-output":
                        self.assertIn("source-built CLI is missing", result.stderr)


class PackagedCandidateCli(unittest.TestCase):
    def test_macos_bundle_tests_use_the_release_cli_built_from_this_source(self):
        script = "scripts/ci/macos-bundle.sh"
        fixture = Fixture(self, [script, "VERSION", "DEV_VERSION"],
                          extra_tools=("npm", "python3", "tmux", "ditto", "git"))
        (fixture.workspace / "client").mkdir()
        result, calls = fixture.run(["bash", str(fixture.workspace / script)], {"ZERUS_RUN_TESTS": "true"})
        self.assertEqual(result.returncode, 0, result.stderr)
        cargo_calls = cargo(calls)
        tests = [call for call in cargo_calls if is_tauri_test(call)]
        self.assertEqual(len(tests), 1)
        expected = fixture.workspace / "target/release/hgs"
        self.assertEqual(Path(tests[0]["HGS_TEST_BIN"]).resolve(), expected.resolve())
        self.assertTrue(tests[0]["cli_ok"])
        builds = [call for call in cargo_calls if call["args"][:1] == ["build"]]
        # Only the existing release builds; no extra debug CLI.
        self.assertEqual([call["args"] for call in builds],
                         [["build", "--locked", "--release"],
                          ["build", "--locked", "--release", "--manifest-path", "services/mobile/connector/Cargo.toml"]])
        self.assertLess(cargo_calls.index(builds[0]), cargo_calls.index(tests[0]))

        version = (ROOT / "DEV_VERSION").read_text().strip()
        result, calls = fixture.run(["bash", str(fixture.workspace / script)],
                                    {"ZERUS_RUN_TESTS": "false", "ZERUS_RELEASE_CHANNEL": "dev",
                                     "ZERUS_DEV_BUILD_NUMBER": "7", "ZERUS_RELEASE_VERSION": version + "-dev.7"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse([call for call in cargo(calls) if call["args"][:1] == ["test"]])

    def test_arch_package_checks_use_the_release_cli_that_build_made(self):
        for recipe, source in (("packaging/aur/zerus-git/PKGBUILD", "zerus"),
                               ("packaging/aur/zerus/PKGBUILD.in", "zerus-1.2.3")):
            with self.subTest(recipe=recipe):
                fixture = Fixture(self, [recipe], extra_tools=("npm", "python"))
                srcdir = fixture.root / "src dir"
                (srcdir / source / "client").mkdir(parents=True)
                program = ('set -euo pipefail; source "$1"; pkgver=1.2.3; srcdir="$2"; build; check')
                result, calls = fixture.run(["bash", "-c", program, "recipe", str(fixture.workspace / recipe),
                                             str(srcdir)])
                self.assertEqual(result.returncode, 0, result.stderr)
                cargo_calls = cargo(calls)
                tests = [call for call in cargo_calls if is_tauri_test(call)]
                self.assertEqual(len(tests), 1)
                self.assertEqual(Path(tests[0]["HGS_TEST_BIN"]).resolve(),
                                 (srcdir / source / "target/release/hgs").resolve())
                self.assertTrue(tests[0]["cli_ok"])
                self.assertFalse([call for call in cargo_calls if is_cli_build(call)], "no extra debug CLI build")


if __name__ == "__main__":
    unittest.main()
