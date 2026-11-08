"""The local CI command must expose failures without borrowing a live profile."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("local_linux_ci", ROOT / "scripts/ci/local-linux.py")
CI = importlib.util.module_from_spec(SPEC)
with patch.object(sys, "path", [str(ROOT / "scripts/ci"), *sys.path]):
    SPEC.loader.exec_module(CI)


class WorkflowBodies(unittest.TestCase):
    def test_cli_continues_after_smoke_and_keeps_exact_python_preparation(self):
        body = (ROOT / "scripts/ci/cli.sh").read_text()
        units = dict(CI.cli_units(body))
        self.assertIn("bash tests/test_hgs.sh", units["CLI shell smoke"])
        self.assertNotIn("tests/test_hgs.sh", units["CLI Python integration"])
        self.assertIn('export HGS_TEST_BIN="$PWD/target/debug/hgs"', units["CLI Python integration"])
        self.assertIn("--test voice_recovery_fixture --no-run", units["CLI Python integration"])
        self.assertIn("export HGS_TEST_SPEECH_RECOVERY_BIN", units["CLI Python integration"])
        self.assertIn("python3 scripts/ci/python-tests.py", units["CLI Python integration"])
        self.assertIn("cargo build --release --locked", units["CLI release build"])
        changed = body.replace("python3 scripts/ci/python-tests.py", "python3 scripts/ci/replacement.py")
        with self.assertRaises(ValueError):
            CI.cli_units(changed)

    def test_unknown_jobs_and_partial_coverage_cannot_be_green(self):
        workflow = {"jobs": {
            "quick": {"runs-on": "ubuntu-24.04", "steps": [
                {"name": "real check", "run": "exit 9"},
                {"name": "needs network", "run": "curl https://example.invalid"}]},
            "new-linux-job": {"runs-on": "ubuntu-24.04", "steps": [{"run": "true"}]}}}
        jobs = CI.plan(workflow, "")
        self.assertEqual(jobs[0]["steps"][0]["body"], "exit 9")
        self.assertNotIn("body", jobs[0]["steps"][1])
        jobs[0]["steps"][0]["status"] = "pass"
        self.assertEqual(CI.summarize(jobs[0]["steps"]), "not run")
        jobs[0]["steps"][0]["status"] = "fail"
        self.assertEqual(CI.summarize(jobs[0]["steps"]), "fail")
        self.assertEqual(jobs[1]["status"], "not run")

    def test_sha_must_not_be_a_moving_branch_or_option(self):
        for value in ("main", "HEAD~1", "--help", "1234567;echo bad"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                CI.resolve(ROOT, value)

    def test_workflow_bodies_remain_exact_and_unknown_conditions_refuse(self):
        body = "printf '%s\\n' a\nexit 7\n"
        workflow = {"jobs": {"quick": {"runs-on": "ubuntu-24.04", "steps": [
            {"run": body}, {"run": "true", "if": "some.new.condition"},
            {"uses": "unknown/action@123"}]}}}
        rows = CI.plan(workflow, "")[0]["steps"]
        self.assertEqual(rows[0]["body"], body)
        self.assertEqual(rows[0]["body_sha256"], CI.digest(body.encode()))
        self.assertNotIn("body", rows[1])
        self.assertNotIn("body", rows[2])

    def test_online_runs_the_pinned_tmux_body_instead_of_the_host_version(self):
        step = {"run": 'bash scripts/ci/install-tmux.sh "$RUNNER_TEMP/tmux"\n'}
        self.assertIsNone(CI.infrastructure(step, online=True))
        self.assertEqual(CI.infrastructure(step, online=False)[0], "not run")

    def test_only_complete_known_infrastructure_bodies_are_substituted(self):
        known = [
            'python3 scripts/ci/ubuntu-disk.py prepare',
            'python3 scripts/ci/ubuntu-disk.py report after-root-tests',
            CI.RUST_CACHE.format(toolchain=' +1.99.1'),
            'rustup toolchain install 1.99.1 --profile minimal --component clippy',
            *CI.APT_PREREQUISITES,
        ]
        for body in known:
            self.assertEqual(CI.infrastructure(dict(run=body), online=True)[0], 'local')
            for suffix in ('; printf MISSED; exit 19', '\nprintf MISSED\nexit 19\n'):
                with self.subTest(body=body, suffix=suffix):
                    result = CI.infrastructure(dict(name='Rust cache identity', run=body + suffix), online=True)
                    self.assertTrue(result is None or result[0] == 'not run', result)
        self.assertIsNone(CI.infrastructure(dict(name='Rust cache identity', run='printf MISSED; exit 19'), online=True))
        mixed = dict(uses='actions/setup-node@123', run='exit 19', with_={})
        self.assertEqual(CI.infrastructure(mixed, online=True)[0], 'not run')

    def test_container_does_not_inherit_the_namespace_only_home_paths(self):
        with tempfile.TemporaryDirectory(prefix='container-ci-') as temporary:
            root = Path(temporary)
            runner = SimpleNamespace(output=root)
            container = CI.Container(runner, 'archlinux@sha256:fixture')
            container.identity = 'original-private-container'
            script = root / 'step.sh'; script.write_text('true\n')
            argv = container.command(script, Path('.'), dict(XDG_STATE_HOME='/home/test/.local/state',
                                     XDG_RUNTIME_DIR='/tmp/r', RUSTUP_HOME='/must-not-inherit'))
            env = dict(arg.split('=', 1) for i, arg in enumerate(argv) if i and argv[i - 1] == '--env')
            self.assertEqual(env['XDG_STATE_HOME'], '/home/user/.local/state')
            self.assertEqual(env['XDG_RUNTIME_DIR'], '/home/user/.runtime')
            self.assertNotIn('RUSTUP_HOME', env)


@unittest.skipUnless(sys.platform == "linux" and shutil.which("bwrap") and shutil.which("dbus-run-session")
                     and hasattr(os, "sched_getaffinity"), "requires unprivileged Linux namespaces")
class RealIsolation(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lci-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.args = SimpleNamespace(output=self.base / "out", cache=self.base / "cache",
                                    target=None, workers=1, max_load=99999, min_free_gib=0,
                                    online=False, slirp=None, android_cache=None, tool_bin=[])
        self.args.plan, self.args.match = False, None
        self.report = {"jobs": [], "completed": False}
        self.runner = CI.Runner(self.args, self.report)
        for folder in (self.runner.output, self.runner.cache / "cargo", self.runner.workspace,
                       self.runner.target, self.runner.home, self.runner.tools, self.runner.output / "harness"):
            folder.mkdir(parents=True, exist_ok=True)
        self.runner.rustup = self.base / "rustup"
        self.runner.rustup.mkdir()
        self.runner.rustup_source = self.runner.rustup

    def test_real_failure_does_not_hide_later_check_and_logs_are_hashed(self):
        job = {"id": "quick", "steps": []}
        for body, expected in (("printf 'first-failure\\n'; exit 19", "fail"),
                               ("printf 'later-check\\n'", "pass")):
            row = dict(name=expected, index=len(job["steps"]), body=body, directory=".", env={}, timeout=10)
            job["steps"].append(row)
            self.runner.run(job, row)
            self.assertEqual(row["status"], expected)
            self.assertEqual(row["log_sha256"], CI.digest((self.runner.output / row["log"]).read_bytes()))
        self.assertEqual(job["steps"][0]["exit_code"], 19)
        self.assertEqual(CI.summarize(job["steps"]), "fail")

    def test_a_compound_cache_step_executes_and_cannot_make_a_job_pass(self):
        workflow = {'jobs': {'quick': {'runs-on': 'ubuntu-24.04', 'steps': [
            {'name': 'real check', 'run': 'printf first > first'},
            {'name': 'Rust cache identity', 'run': 'echo cache-identity; printf MISSED-CHECK > missed; exit 19'},
            {'name': 'later check', 'run': 'printf later > later'},
        ]}}}
        job = CI.plan(workflow, '')[0]
        CI.run_job(self.runner, self.args, self.report, job)
        self.assertEqual(job['status'], 'fail')
        self.assertEqual(job['steps'][1]['exit_code'], 19)
        self.assertEqual((self.runner.workspace / 'missed').read_text(), 'MISSED-CHECK')
        self.assertEqual((self.runner.workspace / 'later').read_text(), 'later')

    def test_private_home_pid_net_and_bus_without_inherited_selection(self):
        canary = self.base / "owner-canary"
        canary.write_text("must not appear inside private /tmp")
        host = {name: os.readlink(f"/proc/self/ns/{name}") for name in ("pid", "net")}
        body = """python3 - <<'PY'
import json,os,pathlib
assert not pathlib.Path(CANARY).exists()
assert not any(k.startswith('HGS_') for k in os.environ)
assert 'ZERUS_CLIENT_DESKTOP_SERVICE' not in os.environ
assert os.environ['HOME']=='/home/test'
assert os.environ['TMUX_TMPDIR']=='/tmp/t'
assert pathlib.Path('/tmp/t').is_dir()
assert os.environ['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')
s=pathlib.Path('/proc/self/status').read_text()
assert 'NoNewPrivs:\\t1' in s
assert 'CapEff:\\t0000000000000000' in s
assert os.getuid()==EXPECTED_UID
print(json.dumps({k:os.readlink('/proc/self/ns/'+k) for k in ('pid','net')}))
PY
""".replace("CANARY", repr(str(canary))).replace("EXPECTED_UID", str(os.getuid()))
        row = dict(name="isolation", index=0, body=body, directory=".", env={}, timeout=10)
        with patch.dict(os.environ, HGS_STATE_DIR="/must-not-inherit", ZERUS_CLIENT_DESKTOP_SERVICE="1"):
            self.runner.run({"id": "quick"}, row)
        self.assertEqual(row["status"], "pass", (self.runner.output / row["log"]).read_text())
        observed = json.loads((self.runner.output / row["log"]).read_text().splitlines()[-1])
        self.assertNotEqual(host["pid"], observed["pid"])
        self.assertNotEqual(host["net"], observed["net"])

    def test_android_gradle_metadata_and_notices_share_the_private_home(self):
        self.runner.android_enabled = True
        self.runner.android_cache.mkdir(parents=True)
        self.runner.rustup_source = self.runner.rustup
        (self.runner.workspace / "notices.py").write_bytes((ROOT / "scripts/android-notices.py").read_bytes())
        body = """python3 - <<'PY'
import importlib.util,json,os,pathlib
home = pathlib.Path(os.environ['HOME'])
assert os.environ.get('GRADLE_USER_HOME') == str(home / '.gradle')
cache = pathlib.Path(os.environ['GRADLE_USER_HOME']) / 'caches/modules-2/files-2.1'
module = cache / 'org.example/metadata/1.0/fixture/metadata-1.0.module'
module.parent.mkdir(parents=True)
module.write_text(json.dumps({'variants': [{'name': 'common'}]}))
spec = importlib.util.spec_from_file_location('notices', 'notices.py')
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)
entries = notices.Libraries(home / '.gradle/caches/modules-2/files-2.1', True).components(
    {'configuration': notices.CONFIGURATION, 'components': [
        {'id': 'org.example:metadata:1.0', 'categories': ['library'], 'files': [], 'redirects': []}]})
assert 'org.example:metadata:1.0' in entries
print('Gradle and notices use the same private metadata cache')
PY
"""
        row = dict(name="Android private metadata", index=0, body=body, directory=".", env={}, timeout=10)
        self.runner.run({"id": "android"}, row)
        self.assertEqual(row["status"], "pass", (self.runner.output / row["log"]).read_text())

    def test_timeout_reaps_the_original_namespace_and_never_touches_other_child(self):
        unrelated = subprocess.Popen(["sleep", "20"])
        def settle():
            if unrelated.poll() is None:
                unrelated.kill()
                unrelated.wait(timeout=5)
        self.addCleanup(settle)
        row = dict(name="timeout", index=0, body="sleep 20 & wait", directory=".", env={}, timeout=0.3)
        self.runner.run({"id": "quick"}, row)
        self.assertEqual(row["exit_code"], 124)
        self.assertIsNone(unrelated.poll())
        unrelated.terminate()
        unrelated.wait(timeout=5)

    def test_synthetic_root_is_trusted_without_mapping_host_root_or_reading_owner_files(self):
        installer = ROOT / "scripts/install-mobile-connector-binary.py"
        (self.runner.workspace / "installer.py").write_bytes(installer.read_bytes())
        canary = self.base / "outside-selected-binds"
        canary.write_text("Private fixture data must not appear inside the namespace")
        host_root = Path("/").stat()
        body = """python3 - <<'PY'
import importlib.util,os,pathlib
s=importlib.util.spec_from_file_location('installer','installer.py')
m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
m.trusted_parents(pathlib.Path('/mnt/workspace'))
assert pathlib.Path('/').stat().st_uid in (0,os.geteuid())
inside_root = pathlib.Path('/').stat()
assert (inside_root.st_dev, inside_root.st_ino) != HOST_ROOT
assert not pathlib.Path(CANARY).exists()
assert all(line.split()[4] != '/w' for line in pathlib.Path('/proc/self/mountinfo').read_text().splitlines())
PY
""".replace("HOST_ROOT", repr((host_root.st_dev, host_root.st_ino))).replace("CANARY", repr(str(canary)))
        row = dict(name="root permissions", index=0, body=body, directory=".", env={}, timeout=10)
        self.runner.run({"id": "quick"}, row)
        self.assertEqual(row["status"], "pass", (self.runner.output / row["log"]).read_text())

    def test_synthetic_root_contract_accepts_a_private_toolchain_mount_under_w(self):
        # Only the namespace destination changes; the source stays in our private fixture.
        self.runner.rustup = Path("/") / "w" / "fixture-private-toolchain"
        self.test_synthetic_root_is_trusted_without_mapping_host_root_or_reading_owner_files()

    def test_synthetic_root_contract_refuses_a_broad_mount_under_w(self):
        # Expose only synthetic fixture data, never the host's /w tree.
        source = self.base / "unselected-w"
        source.mkdir(mode=0o700)
        (source / "unselected-file").write_text("Private synthetic mount")
        original = self.runner.command
        def broad_mount(*args, **kwargs):
            command = original(*args, **kwargs)
            index = command.index("--")
            return command[:index] + ["--ro-bind", str(source), "/w"] + command[index:]
        with patch.object(self.runner, "command", broad_mount):
            with self.assertRaisesRegex(AssertionError, "'fail' != 'pass'") as refusal:
                self.test_synthetic_root_is_trusted_without_mapping_host_root_or_reading_owner_files()
        self.assertIn("Traceback (most recent call last)", str(refusal.exception))

    def test_job_checkouts_do_not_share_mutable_files(self):
        self.runner.pristine.mkdir()
        (self.runner.pristine / "source").write_text("original")
        self.runner.new_checkout({"id": "first"})
        first = self.runner.workspace / "source"
        first.write_text("mutated by test")
        self.runner.new_checkout({"id": "second"})
        self.assertEqual((self.runner.workspace / "source").read_text(), "original")
        self.assertEqual((self.runner.pristine / "source").read_text(), "original")
        self.assertNotEqual(first.stat().st_ino, (self.runner.workspace / "source").stat().st_ino)

    def network_fixture(self, refuse_first):
        self.args.online = True
        helper = self.base / "setup-helper"
        helper.write_text("""#!/usr/bin/python3
import os,pathlib,sys
p=pathlib.Path(__file__).with_suffix('.count')
n=int(p.read_text())+1 if p.exists() else 1
p.write_text(str(n))
if REFUSE and n==1:sys.exit(7)
def fd(name):return int(next(a.split('=',1)[1] for a in sys.argv if a.startswith(name+'=')))
os.write(fd('--ready-fd'),b'1')
os.read(fd('--exit-fd'),1)
""".replace("REFUSE", repr(refuse_first)))
        helper.chmod(0o700)
        self.args.slirp = helper
        (self.runner.output / "resolv.conf").write_text("nameserver 10.0.2.3\n")
        return helper.with_suffix(".count")

    def test_pre_entry_setup_refusal_never_runs_the_body_twice(self):
        count = self.network_fixture(True)
        row = dict(name="pre-entry refusal", index=0, body="echo entry >> entered", directory=".", env={}, timeout=10)
        self.runner.run({"id": "quick"}, row)
        self.assertEqual(row["status"], "pass", (self.runner.output / row["log"]).read_text())
        self.assertEqual(count.read_text(), "2")
        self.assertEqual((self.runner.workspace / "entered").read_text(), "entry\n")
        self.assertEqual(len(row["preparation_refusals"]), 1)
        self.assertTrue(row["preparation_refusals"][0]["before_release"])

    def test_failure_after_release_is_uncertain_and_never_retried(self):
        count = self.network_fixture(False)
        original = CI.Network.connect
        def fail_after_release(network):
            original(network)
            raise RuntimeError("injected after the real release write")
        row = dict(name="post-release failure", index=0, body="echo entry >> entered", directory=".", env={}, timeout=10)
        with patch.object(CI.Network, "connect", fail_after_release):
            self.runner.run({"id": "quick"}, row)
        self.assertEqual(row["status"], "fail")
        self.assertTrue(row["execution_started"])
        self.assertEqual(count.read_text(), "1")
        self.assertFalse(row["preparation_refusals"][0]["before_release"])


if __name__ == "__main__":
    unittest.main()
