"""A failed synthetic send keeps its original bounded diagnostic after cleanup."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import test_codex_input_native as native
import test_windows_host as windows


class CodexRefusalArtifacts(unittest.TestCase):
    def diagnostic(self, root, **updates):
        value = dict(version=1, agent="codex", run_id="fixture-run", conversation_id="",
                     cursor_x=2, cursor_y=20, cursor_stable=False, cursor_row="",
                     rows=[dict(row=20, shape=dict(blank=False, dim=True))])
        value.update(updates)
        source = root / "state/input_diagnostics/codex.json"
        source.parent.mkdir(parents=True)
        raw = json.dumps(value).encode()
        source.write_bytes(raw)
        (root / "state/unrelated.json").write_text('"unrelated fixture data"')
        return raw

    def refused(self, case, startup):
        case.env, case.name, case.run_id = {}, "codex/fixture/refusal", "fixture-run"
        case.screen = lambda: "› Ask Codex to do anything"
        case.until = lambda *args: True
        result = subprocess.CompletedProcess([], 1, "", "hgs: terminal has a draft, dialog, or unrecognized prompt")
        with patch.object(native.subprocess, "run", return_value=result) as send:
            with self.assertRaisesRegex(AssertionError, "terminal has a draft"):
                if startup:
                    case.deliver_first()
                else:
                    case.send(dict(request_id="fixture-request", text="Synthetic fixture message"))
        send.assert_called_once()  # Preserving evidence never retries a refused input.

    def cleanup(self, root, repo, kind):
        if kind == "startup":
            case = native.NativeCodexStartup("test_fresh_native_launch_reaches_its_first_turn_through_the_real_hooks")
            case.root = root
            self.refused(case, True)
            with patch.object(native.test_pause, "REPO", repo):
                case.remove_fixture_root()
        elif kind == "input":
            case = native.NativeCodexInput("test_leading_absolute_path_is_literal_message_text")
            case.root, case.state = root, root / "state"
            case.temp = SimpleNamespace(cleanup=lambda: shutil.rmtree(root))
            case.tmux = lambda *args, **kwargs: ""
            self.refused(case, False)
            with patch.object(native.test_input, "REPO", repo):
                case.tearDown()
        else:
            case = windows.HostCase()
            case.root = root
            case.temp = SimpleNamespace(cleanup=lambda: shutil.rmtree(root))
            with patch.object(windows, "REPO", repo):
                case.cleanup_temp()
        self.assertFalse(root.exists(), "the private state must still be cleaned")
        return sorted((repo / "artifacts/test-results/python/input-diagnostics").glob("*.json"))

    def test_forced_startup_refusal_survives_private_cleanup_in_uploaded_artifacts(self):
        self.preserve("startup")

    def test_forced_sibling_input_refusal_survives_private_cleanup_in_uploaded_artifacts(self):
        self.preserve("input")

    def test_windows_native_host_cleanup_preserves_the_same_bounded_diagnostic(self):
        self.preserve("windows")

    def preserve(self, kind):
        with tempfile.TemporaryDirectory(prefix="zcd-") as folder:
            root, repo = Path(folder) / "fixture", Path(folder) / "repo"
            raw = self.diagnostic(root)
            artifacts = self.cleanup(root, repo, kind)
            self.assertEqual(len(artifacts), 1, "the original diagnostic was lost before upload")
            self.assertEqual(artifacts[0].read_bytes(), raw)

    def test_oversized_diagnostic_is_not_uploaded_and_the_refusal_still_fails(self):
        with tempfile.TemporaryDirectory(prefix="zcd-") as folder:
            root, repo = Path(folder) / "fixture", Path(folder) / "repo"
            self.diagnostic(root, cursor_row="x" * (16 * 1024))
            self.assertEqual(self.cleanup(root, repo, "startup"), [])

    def test_unwritable_artifacts_do_not_replace_the_refusal_or_block_cleanup(self):
        with tempfile.TemporaryDirectory(prefix="zcd-") as folder:
            root, repo = Path(folder) / "fixture", Path(folder) / "repo"
            self.diagnostic(root)
            repo.mkdir()
            (repo / "artifacts").write_text("a file blocks the artifact directory")
            self.assertEqual(self.cleanup(root, repo, "startup"), [])

    def test_missing_diagnostic_never_blocks_the_original_failure_or_cleanup(self):
        with tempfile.TemporaryDirectory(prefix="zcd-") as folder:
            root, repo = Path(folder) / "fixture", Path(folder) / "repo"
            root.mkdir()
            self.assertEqual(self.cleanup(root, repo, "startup"), [])

    def test_repeated_case_ids_keep_distinct_original_samples(self):
        with tempfile.TemporaryDirectory(prefix="zcd-") as folder:
            root, repo = Path(folder) / "fixture", Path(folder) / "repo"
            first = self.diagnostic(root)
            self.cleanup(root, repo, "startup")
            second = self.diagnostic(root, cursor_stable=True, cursor_row="› Synthetic placeholder")
            artifacts = self.cleanup(root, repo, "startup")
            self.assertEqual(len(artifacts), 2)
            self.assertEqual({p.read_bytes() for p in artifacts}, {first, second})

    @unittest.skipUnless(os.name == "posix", "source-link control needs unprivileged symlinks")
    def test_a_link_outside_the_private_fixture_is_never_uploaded(self):
        with tempfile.TemporaryDirectory(prefix="zcd-") as folder:
            root, repo = Path(folder) / "fixture", Path(folder) / "repo"
            raw = self.diagnostic(root)
            outside = Path(folder) / "outside-synthetic.json"
            outside.write_bytes(raw)
            source = root / "state/input_diagnostics/codex.json"
            source.unlink()
            source.symlink_to(outside)
            self.assertEqual(self.cleanup(root, repo, "startup"), [])

    def test_the_exact_sixteen_kib_boundary_is_preserved(self):
        with tempfile.TemporaryDirectory(prefix="zcd-") as folder:
            root, repo = Path(folder) / "fixture", Path(folder) / "repo"
            raw = self.diagnostic(root)
            raw += b" " * (16 * 1024 - len(raw))
            (root / "state/input_diagnostics/codex.json").write_bytes(raw)
            artifacts = self.cleanup(root, repo, "startup")
            self.assertEqual(len(artifacts), 1)
            self.assertEqual(artifacts[0].read_bytes(), raw)

    @unittest.skipUnless(os.name == "posix", "directory-link control needs unprivileged symlinks")
    def test_a_linked_state_directory_never_reads_an_outside_sample(self):
        with tempfile.TemporaryDirectory(prefix="zcd-") as folder:
            root, outside, repo = (Path(folder) / name for name in ("fixture", "outside", "repo"))
            self.diagnostic(outside)
            root.mkdir()
            (root / "state").symlink_to(outside / "state", target_is_directory=True)
            self.assertEqual(self.cleanup(root, repo, "startup"), [])
