"""WhiteWh fork, task #65 (owner, 2026-11-06): the fork's Windows zip ships the
Tauri app as hgs-tray.exe with the fork's identity and feed, and keeps the Qt
GUI for one release as qt/zerus-qt.exe, a fallback started by hand."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

import test_windows_publication as publication
from test_windows_publication import PAIR, bundle, manifest, pinned, portable, use_pin

ROOT = Path(__file__).resolve().parents[1]
FEED = "https://github.com/WhiteWh/zerus/releases/latest/download/zerus-fork-feed.json"


class ForkInspectTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="zerus-fork-zip-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        use_pin(self, pinned())
        self.files = {name: PAIR.get(name, b"x") for name in portable.NEXT_REQUIRED}
        self.fallback = {f"qt/{name}": b"Qt" for name in portable.QT_FALLBACK_REQUIRED}
        self.files.update(self.fallback)
        self.files["qt/licenses/qt/LGPL-3.0-only.txt"] = b"LGPL"
        self.info = manifest(desktop="tauri", webview2_runtime="evergreen", mobile_connector_bundled=True,
                             fork={"repository": "WhiteWh/zerus", "build": 41, "feed": FEED},
                             qt_fallback={"executable": "qt/zerus-qt.exe", "qt_version": "6.10.3"})
        del self.info["qt_version"]

    def test_a_declared_qt_fallback_in_its_folder_is_accepted(self):
        self.assertEqual(portable.inspect(bundle(self.directory / "ok.zip", self.files, self.info)), self.info)

    def test_qt_files_stay_in_the_declared_fallback_folder(self):
        undeclared = {key: value for key, value in self.info.items() if key != "qt_fallback"}
        with self.assertRaisesRegex(ValueError, "must not contain Qt"):
            portable.inspect(bundle(self.directory / "undeclared.zip", self.files, undeclared))
        for name in ["Qt6Core.dll", "platforms/qwindows.dll", "licenses/qt/LGPL-3.0-only.txt", "other/Qt6Gui.dll"]:
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "must not contain Qt"):
                portable.inspect(bundle(self.directory / "outside.zip", {**self.files, name: b"Qt"}, self.info))

    def test_the_fallback_is_complete_and_never_a_second_desktop_or_cli(self):
        for name in self.fallback:
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "Qt fallback lacks"):
                files = {key: value for key, value in self.files.items() if key != name}
                portable.inspect(bundle(self.directory / "missing.zip", files, self.info))
        for name in ["qt/hgs.exe", "qt/hgs-tray.exe"]:
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "only zerus-qt.exe"):
                portable.inspect(bundle(self.directory / "second.zip", {**self.files, name: b"x"}, self.info))
        for declaration in [{"executable": "zerus-qt.exe", "qt_version": "6.10.3"},
                            {"executable": "qt/zerus-qt.exe", "qt_version": "six"}, "qt/zerus-qt.exe"]:
            with self.subTest(declaration=declaration), self.assertRaisesRegex(ValueError, "Qt fallback declaration"):
                portable.inspect(bundle(self.directory / "bad.zip", self.files, {**self.info, "qt_fallback": declaration}))


class ForkPackageTests(unittest.TestCase):
    setUp = publication.PackageTests.setUp
    run_script = publication.PackageTests.run_script

    def fork(self, output, *extra):
        arguments = ["--connector", self.directory / "zerus-mobile-connector.exe", "--cli", self.directory / "hgs.exe",
                     "--lore", self.directory / "lore.exe",
                     "--gui", self.directory / "hgs-tray.exe", "--output", output, "--crt-dir", self.crt,
                     "--rust-licenses", self.rust, "--frontend-licenses", self.frontend, "--conpty", self.conpty, *extra]
        environment = {key: value for key, value in os.environ.items() if not key.startswith(("QT_", "HGS_", "ZERUS_FORK_"))}
        environment["SOURCE_DATE_EPOCH"] = "1760000000"
        return self.run_script("package-windows.py", *map(str, arguments), cwd=self.directory, env=environment)

    def test_the_fork_zip_names_its_build_and_feed_and_the_fork_readme(self):
        output = self.directory / "fork.zip"
        result = self.fork(output, "--fork", "--fork-build", "41", "--fork-feed", FEED)
        self.assertEqual(result.returncode, 0, result.stderr)
        info = portable.inspect(output)
        self.assertEqual(info["fork"], {"repository": "WhiteWh/zerus", "build": 41, "feed": FEED})
        self.assertEqual(info["desktop"], "tauri")
        self.assertNotIn("qt_fallback", info)
        with zipfile.ZipFile(output) as archive:
            readme = archive.read(f"{portable.TOP}/README.txt").decode("utf-8")
            self.assertEqual(archive.read(f"{portable.TOP}/hgs-tray.exe"), b"tested tauri gui")
        self.assertIn("WhiteWh fork", readme)
        self.assertIn("41;", readme)
        self.assertIn(FEED, readme)
        self.assertIn("This zip has no Qt fallback.", readme)
        self.assertNotRegex(readme, r"@[A-Z_]+@")
        self.assertIn("\r\n", readme)
        # An experimental build has neither a number nor a feed.
        experimental = self.directory / "experimental.zip"
        self.assertEqual(self.fork(experimental, "--fork").returncode, 0)
        self.assertEqual(portable.inspect(experimental)["fork"], {"repository": "WhiteWh/zerus", "build": None, "feed": ""})

    def test_a_fork_identity_outside_the_forks_releases_is_refused(self):
        for extra, words in [(["--fork-build", "41", "--fork-feed", "https://example.com/feed.json"], "release asset"),
                             (["--fork-build", "41"], "both a build number and a feed"),
                             (["--fork-build", "-3", "--fork-feed", FEED], "positive number")]:
            with self.subTest(extra=extra):
                output = self.directory / "refused.zip"
                result = self.fork(output, "--fork", *extra)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(words, result.stderr)
                self.assertFalse(output.exists())
        result = self.fork(self.directory / "plain.zip", "--qt-fallback", self.directory / "hgs-tray.exe")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("belongs to a fork zip", result.stderr)


class ForkWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.text = (ROOT / ".github/workflows/fork-windows.yml").read_text(encoding="utf-8")
        steps = self.text.split("\n      - ")
        self.step = lambda words: next(step for step in steps if words in step)
        self.index = lambda words: next(index for index, step in enumerate(steps) if words in step)

    def test_the_fork_builds_and_packages_the_tauri_app_with_its_identity(self):
        build = self.step("name: Build the Tauri Windows desktop with the fork channel")
        self.assertIn("npm run tauri -- build --ci --no-bundle --config src-tauri/tauri.windows.conf.json --features video", build)
        package = self.step("name: Portable zip from the tested executables")
        for part in ["package-windows.py --fork", "--lore target/lore/release/lore.exe", "--gui target/windows-next/release/hgs-tray.exe",
                     "--mobile-relay services/mobile/relay/target/release/zerus-relay.exe",
                     "--qt-fallback .ci-build/gui/hgs-tray.exe", '--qt-root "$QT_ROOT_DIR"',
                     "--frontend-licenses artifacts/windows-frontend-licenses",
                     "check-windows-portable.py artifacts/zerus-windows-x86_64.zip --run --selftest"]:
            self.assertIn(part, package)
        self.assertNotIn("package-windows-qt.py", self.text)
        self.assertIn("ZERUS_FORK_BUILD: ${{ !startsWith(github.ref, 'refs/tags/whitewh-exp-') && github.run_number || '' }}", self.text)
        self.assertIn("ZERUS_FORK_FEED_URL:", self.text)
        # The prerequisites and the tests come before the zip.
        zip_step = self.index("name: Portable zip from the tested executables")
        for words in ["name: Install the locked Zerus Next frontend", "name: Frontend runtime license notices",
                      "name: WebView2 Evergreen prerequisite", "name: Desktop build", "name: Offscreen Qt suites",
                      "name: Fork update channel and the Qt-to-Tauri handoff",
                      "name: Build the Tauri Windows desktop with the fork channel"]:
            self.assertLess(self.index(words), zip_step, words)
        self.assertLess(zip_step, self.index("name: Update feed"))


if __name__ == "__main__":
    unittest.main()

