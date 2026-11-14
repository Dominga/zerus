"""Offline package-policy and loader-output controls; no real apt, sudo or SDK use."""
import ast
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("emulator_runtime_driver", ROOT / "scripts/ci/android-project-ui.py")
UI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UI)
VERSIONS = {
    "libpulse0": "1:16.1+dfsg1-2ubuntu10.1",
    "libnss3": "2:3.98-1ubuntu0.3",
    "libnspr4": "2:4.35-1.1build1",
    "libpng16-16t64": "1.6.43-5ubuntu0.7",
    "libexpat1": "2.6.1-2ubuntu0.6",
    "libdrm2": "2.4.125-1ubuntu0.1~24.04.2",
    "libxi6": "2:1.8.1-1build1",
    "libxext6": "2:1.3.4-1build2",
    "libxkbfile1": "1:1.1.0-1build4"
}
VERSION = VERSIONS["libpulse0"]


class EmulatorRuntimeTests(unittest.TestCase):
    def test_runtime_package_receipt_requires_every_exact_version_once(self):
        good = "".join(name + "=" + version + "\n" for name, version in VERSIONS.items())
        self.assertEqual(UI.verified_runtime_packages(good), VERSIONS)
        for text in ("", good.replace(VERSION, VERSION + ".new"), good + good.splitlines()[0] + "\n",
                     "\n".join(good.splitlines()[1:]) + "\n", good + "unknown=1\n", "x" * 65537):
            with self.subTest(text=text[:100]), self.assertRaises(AssertionError):
                UI.verified_runtime_packages(text)

    def test_loader_requires_resolved_pulse_and_refuses_every_missing_dependency(self):
        good = "libpulse.so.0 => /lib/x86_64-linux-gnu/libpulse.so.0 (0x00007f1234567000)\nlibc.so.6 => /lib/libc.so.6 (0x00007f1111111000)\n"
        UI.emulator_runtime_libraries(good)
        for output in ("", "statically linked\n", good.replace("/lib/x86_64-linux-gnu/libpulse.so.0 (0x00007f1234567000)", "not found"),
                       good + "libanother.so.1 => not found\n", good + good, "x" * 65537):
            with self.subTest(output=output[:100]), self.assertRaises(AssertionError):
                UI.emulator_runtime_libraries(output)

    def test_workflows_install_only_the_exact_offered_and_installed_package(self):
        blocks = []
        for name in ("android-project-ui.yml", "android-approval-ui.yml"):
            source = (ROOT / ".github/workflows" / name).read_text()
            marker = "      - name: Provision the reviewed emulator runtime libraries\n"
            self.assertIn(marker, source)
            block = source.split(marker, 1)[1].split("      - name: Prepare pinned task-replica", 1)[0]
            blocks.append(block)
            shell = textwrap.dedent(block.split("        run: |\n", 1)[1])
            last = next(reversed(VERSIONS))
            for package, candidate, installed, success, install in (("libpulse0", VERSION, VERSION, True, True),
                    ("libpulse0", VERSION + ".new", VERSION, False, False), ("libpulse0", VERSION, VERSION + ".new", False, True),
                    (last, VERSIONS[last] + ".new", VERSIONS[last], False, False),
                    (last, VERSIONS[last], VERSIONS[last] + ".new", False, True)):
                with self.subTest(workflow=name, package=package, candidate=candidate, installed=installed), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory); tools = root / "bin"; tools.mkdir(); admission = root / "android-project-ui-admission"; admission.mkdir()
                    fake = '''#!/usr/bin/python3
import json,os,sys
from pathlib import Path
name=Path(sys.argv[0]).name
if name=='apt-cache': print('  Candidate: '+json.loads(os.environ['TEST_CANDIDATE'])[sys.argv[-1]])
elif name=='dpkg-query': print(json.loads(os.environ['TEST_INSTALLED'])[sys.argv[-1]],end='')
else:
 assert name=='sudo' and sys.argv[1]=='apt-get'
 with open(os.environ['TEST_CALLS'],'a') as out: out.write(json.dumps(sys.argv[1:])+'\\n')
'''
                    for tool in ("sudo", "apt-cache", "dpkg-query"):
                        path = tools / tool; path.write_text(fake); path.chmod(0o700)
                    calls = root / "calls"
                    env = {"PATH": str(tools) + ":/usr/bin:/bin", "RUNNER_ENVIRONMENT": "github-hosted", "RUNNER_TEMP": str(root),
                        "TEST_CANDIDATE": json.dumps({**VERSIONS, package: candidate}), "TEST_INSTALLED": json.dumps({**VERSIONS, package: installed}), "TEST_CALLS": str(calls)}
                    result = subprocess.run(["bash", "-c", shell], env=env, capture_output=True, text=True, timeout=5)
                    self.assertEqual(result.returncode == 0, success, result.stderr)
                    rows = [json.loads(line) for line in calls.read_text().splitlines()] if calls.exists() else []
                    expected = ["apt-get", "install", "--yes", "--no-install-recommends", *[name + "=" + version for name, version in VERSIONS.items()]]
                    self.assertEqual(expected in rows, install)
        self.assertEqual(blocks[0], blocks[1])

    def test_package_and_all_loader_dependencies_are_verified_before_version_execution(self):
        source = (ROOT / "scripts/ci/android-project-ui.py").read_text(); tree = ast.parse(source)
        main = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "main")
        calls = [node for node in ast.walk(main) if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)]
        stages = {node.args[0].value: node.lineno for node in calls if node.func.id == "run" and node.args and isinstance(node.args[0], ast.Constant)}
        self.assertIn("runtime-packages", stages); self.assertIn("emulator-ldd", stages)
        guard = next(node for node in calls if node.func.id == "emulator_runtime_libraries")
        self.assertLess(stages["runtime-packages"], stages["emulator-ldd"])
        self.assertLess(stages["emulator-ldd"], guard.lineno)
        self.assertLess(guard.lineno, stages["emulator-version"])
        self.assertEqual(UI.RUNTIME_PACKAGES, VERSIONS)
        pins = json.loads((ROOT / "scripts/ci/android-project-ui-pins.json").read_text())
        self.assertEqual({name: row["version"] for name, row in pins["runtime_packages"].items()}, VERSIONS)


if __name__ == "__main__":
    unittest.main()
