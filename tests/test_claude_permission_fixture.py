"""Read the private native config selected by CLAUDE_CONFIG_DIR, with no agent."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
import test_claude_permission_native as native


class PermissionFixtureHome(unittest.TestCase):
    def fixture(self):
        temporary = tempfile.TemporaryDirectory(prefix='zpm-')
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        (root / '.claude').mkdir()
        (root / '.claude.json').write_text(json.dumps({'hasSeenAutoDefaultNudge': False}))
        (root / '.claude/.claude.json').write_text(json.dumps({'hasSeenAutoDefaultNudge': True}))
        (root / '.claude/settings.json').write_text(json.dumps({'permissions': {'defaultMode': 'auto'}}))
        return SimpleNamespace(root=root)

    def test_observes_the_native_selected_home_instead_of_the_stale_legacy_copy(self):
        fixture = self.fixture()
        self.assertEqual(native.NativeClaudeAutoModeOffer.account(fixture), (True, 'auto'))

    def test_missing_selected_master_never_falls_back_to_a_legacy_home(self):
        fixture = self.fixture()
        (fixture.root / '.claude/.claude.json').unlink()
        with self.assertRaises(FileNotFoundError):
            native.NativeClaudeAutoModeOffer.account(fixture)
