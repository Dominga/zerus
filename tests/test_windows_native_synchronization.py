"""Portable controls for the real Windows Codex fixture's Stop synchronization."""
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import Mock, patch

import test_windows_native_agents as native


class ClaudePermissionInputReadiness(unittest.TestCase):
    CARD = dict(question_id='original', question_hash='a' * 64, run_id='run', conversation_id='conversation',
                tool_call_id='tool', agent_id='main', source='claude_tool_approval',
                panel_verified=True, can_answer=True)

    def test_the_actual_terminal_case_waits_before_each_single_original_key(self):
        self.terminal_case(unverified_first=False)

    def test_the_verified_panel_replaces_the_unverified_hook_hash_before_the_guard(self):
        self.terminal_case(unverified_first=True)

    def terminal_case(self, unverified_first):
        case = native.NativeWindowsClaude('test_terminal_answers_end_the_hook')
        with tempfile.TemporaryDirectory() as directory:
            case.project = Path(directory)
            clock = [0.0]
            observations, keys = [], []
            pending = [True]
            case.launch = lambda _: 'claude/fixture/terminal'
            def hook(*_):
                pending[0] = True
                observations.append(clock[0])
                if unverified_first:
                    return dict(self.CARD, panel_verified=False, question_hash='b' * 64)
                return self.CARD.copy()
            case.hook_card = hook
            case.worker = lambda: (123, 'original-birth')
            case.inspect = lambda _: dict(pending_questions=[self.CARD.copy()] if pending[0] else [])
            def send(_, key):
                self.assertGreaterEqual(clock[0] - observations[-1], 0.150)
                keys.append(key['named'])
                pending[0] = False
                (case.project / native.api.BASH_FILE).touch()
            case.keys = send
            def until(what, check):
                item = check()
                self.assertTrue(item, what)
                return item
            case.until = until
            case.alive = lambda *_: False
            case.refused = lambda *_: None
            case.choose = lambda *_: []
            case.channels = lambda _: []
            case.turn_ended = lambda *_: True
            case.capture = lambda _: 'What should Claude do instead?'
            with patch.object(native.time, 'monotonic', lambda: clock[0]), \
                    patch.object(native.time, 'sleep', lambda delay: clock.__setitem__(0, clock[0] + delay)):
                case.test_terminal_answers_end_the_hook()
            self.assertEqual(keys, ['Enter', 'Escape'])

    def test_changed_original_or_unverified_panel_refuses_before_input(self):
        for change in (dict(question_hash='b' * 64), dict(run_id='replacement'),
                       dict(panel_verified=False), dict(can_answer=False)):
            with self.subTest(change=change):
                clock = [0.0]
                read = lambda: dict(self.CARD, **change) if clock[0] > 0 else self.CARD.copy()
                with self.assertRaisesRegex(AssertionError, 'original Claude permission panel changed'):
                    native.wait_claude_permission_guard(self.CARD, read, 0,
                        now=lambda: clock[0], pause=lambda delay: clock.__setitem__(0, clock[0] + delay))
                self.assertEqual(clock[0], 0.040)


class CodexStopSynchronization(unittest.TestCase):
    NAME = "codex/project/question"
    SINCE = 100.0

    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="native-stop-sync-")
        self.addCleanup(temp.cleanup)
        # Call fixture helpers only: no agent, host, CLI or provider is started.
        self.case = native.NativeWindowsCodex()
        self.case.state = Path(temp.name)
        self.case.record = Mock(return_value=dict(run_id="original-run", conversation_id="original-conversation"))
        self.case.inspect = Mock(side_effect=AssertionError("inspect ran ahead of Stop"))

    def database(self):
        database = sqlite3.connect(self.case.state / "events.sqlite3", timeout=0)
        self.addCleanup(database.close)
        database.execute("CREATE TABLE events(seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT)")
        database.execute("CREATE INDEX events_session ON events(name, conversation, seq)")
        database.commit()
        return database

    def stop(self, database, *, name=NAME, conversation="original-conversation", **fields):
        event = dict(type="Stop", at=self.SINCE, run_id="original-run", agent_id="")
        event.update(fields)
        database.execute("INSERT INTO events(name, conversation, payload) VALUES(?,?,?)",
                         (name, conversation, json.dumps(event)))

    def test_no_database_or_binding_never_starts_an_inspector_or_creates_state(self):
        self.assertIsNone(self.case.turn_ended(self.NAME, self.SINCE))
        self.assertEqual(list(self.case.state.iterdir()), [])
        self.case.record.return_value = dict(run_id="original-run")
        self.assertIsNone(self.case.turn_ended(self.NAME, self.SINCE))
        self.case.inspect.assert_not_called()

    def test_old_child_and_other_originals_never_admit_an_inspector(self):
        database = self.database()
        for fields in (dict(at=self.SINCE - 1), dict(agent_id="child"), dict(run_id="replacement"),
                       dict(conversation="replacement"), dict(name="codex/project/other"), dict(type="AgentMessage")):
            self.stop(database, **fields)
        database.commit()
        self.assertIsNone(self.case.turn_ended(self.NAME, self.SINCE))
        self.case.inspect.assert_not_called()

    def test_a_delayed_stop_writer_runs_without_polling_inspectors_then_public_idle_is_required(self):
        database = self.database()
        database.execute("BEGIN EXCLUSIVE")
        self.stop(database)
        # An uncommitted hook stays invisible; even the journal read is nonblocking.
        for _ in range(20):
            self.assertIsNone(self.case.turn_ended(self.NAME, self.SINCE))
        self.case.inspect.assert_not_called()
        database.commit()
        stop = dict(type="Stop", at=self.SINCE)
        busy = dict(events=[stop], activity="busy", phase="working")
        idle = dict(events=[stop], activity="idle", phase="idle")
        self.case.inspect = Mock(side_effect=[busy, idle])
        self.assertIsNone(self.case.turn_ended(self.NAME, self.SINCE))
        self.assertIs(self.case.turn_ended(self.NAME, self.SINCE), idle)
        self.assertEqual(self.case.inspect.call_count, 2)

    def test_the_original_journal_admits_stop_after_the_label_changes(self):
        database = self.database()
        stop = dict(type="Stop", at=self.SINCE)
        idle = dict(events=[stop], activity="idle", phase="idle")
        for journal in ("session:11111111-1111-4111-8111-111111111111", "codex/project/original-label"):
            with self.subTest(journal=journal):
                self.case.record.return_value["journal_name"] = journal
                database.execute("DELETE FROM events")
                self.stop(database, name=journal, agent_id="main")
                database.commit()
                self.case.inspect = Mock(return_value=idle)
                self.assertIs(self.case.turn_ended(self.NAME, self.SINCE), idle)
                self.case.inspect.assert_called_once_with(self.NAME)
                # Journal readiness is still only a gate to the public check.
                self.case.inspect.return_value = dict(events=[], activity="idle", phase="idle")
                self.assertIsNone(self.case.turn_ended(self.NAME, self.SINCE))
                self.case.inspect.return_value = dict(events=[stop], activity="busy", phase="working")
                self.assertIsNone(self.case.turn_ended(self.NAME, self.SINCE))

    def test_an_id_journal_rejects_a_label_stop_and_other_originals(self):
        database = self.database()
        journal = "session:11111111-1111-4111-8111-111111111111"
        self.case.record.return_value["journal_name"] = journal
        self.stop(database)  # A matching event under the label is NOT this journal.
        for fields in (dict(at=self.SINCE - 1), dict(agent_id="child"), dict(run_id="replacement"),
                       dict(conversation="replacement"), dict(type="AgentMessage")):
            self.stop(database, name=journal, **fields)
        database.commit()
        self.assertIsNone(self.case.turn_ended(self.NAME, self.SINCE))
        self.case.inspect.assert_not_called()

    def test_journal_stop_does_not_replace_the_public_stop_check(self):
        database = self.database()
        self.stop(database, agent_id="main")
        database.commit()
        self.case.inspect = Mock(return_value=dict(events=[], activity="idle", phase="idle"))
        self.assertIsNone(self.case.turn_ended(self.NAME, self.SINCE))
        self.case.inspect.assert_called_once_with(self.NAME)


if __name__ == "__main__":
    unittest.main()
