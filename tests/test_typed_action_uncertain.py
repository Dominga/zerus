"""zerus-1wchz: once compact, clear or interrupt may have typed into the
agent, each later failure says "delivery uncertain", so the client never
reads it as a refusal before input. Private tmux socket and fixture state."""
import unittest
import test_input
# Modules, not classes: an imported TestCase would run its own tests here.
import test_clear_context, test_compact_context, test_session_actions

# Replaces every receipt in one directory with a directory, so the command's
# own receipt write after the key fails.
BLOCK_RECEIPTS = 'for f in "$HGS_STATE_DIR"/{}/*.json; do rm -f "$f"; mkdir -p "$f/blocked"; done\n'


def on_keys(case, keys, commands):
    """Runs `commands` in the tmux wrapper just before a send-keys whose last
    arguments are `keys`, as the command sends them."""
    (case.root / 'on-keys.sh').write_text(commands)
    case.script('tmux', '#!/bin/sh\ncase " $* " in *" send-keys "*" ' + keys + ' ") . "$INPUT_FIXTURE/on-keys.sh";; esac\n'
                'exec ' + test_input.TMUX + ' -S "' + str(case.socket) + '" -f /dev/null "$@"\n')


def receipts(case, folder):
    return list((case.state / folder).glob('*.json'))


class TypedCompact(unittest.TestCase):
    setUp = test_compact_context.CompactContext.setUp
    tearDown = test_compact_context.CompactContext.tearDown
    script = test_compact_context.CompactContext.script
    write_record = test_compact_context.CompactContext.write_record
    tmux = test_compact_context.CompactContext.tmux
    wait_for = test_compact_context.CompactContext.wait_for
    received = test_compact_context.CompactContext.received
    compact = test_compact_context.CompactContext.compact

    def test_a_receipt_lost_after_enter_is_delivery_uncertain(self):
        on_keys(self, 'Enter', BLOCK_RECEIPTS.format('compact_receipts'))
        result = self.compact()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('Compact delivery uncertain', result.stderr)
        # Sending Enter completes before the raw-terminal fake journals it.
        self.wait_for(lambda: self.received() == b'/compact\r')
        self.assertEqual(self.received(), b'/compact\r')

    def test_an_unverified_command_whose_record_fails_stays_uncertain(self):
        # The typed command never shows, and its uncertain receipt cannot be
        # written: the error still says the delivery is uncertain.
        on_keys(self, '-l /compact', 'touch "$INPUT_FIXTURE/freeze"\n' + BLOCK_RECEIPTS.format('compact_receipts'))
        result = self.compact()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('Compact delivery uncertain: Native /compact input could not be verified;', result.stderr)
        self.assertEqual(self.received(), b'/compact')

    def test_a_refusal_before_input_stays_definite(self):
        self.record.update(activity='busy', phase='working')
        self.write_record()
        result = self.compact()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn('uncertain', result.stderr.lower())
        self.assertEqual(self.received(), b'')
        self.assertEqual(receipts(self, 'compact_receipts'), [])


class TypedClear(unittest.TestCase):
    setUp = test_clear_context.ClearContext.setUp
    tearDown = test_clear_context.ClearContext.tearDown
    script = test_clear_context.ClearContext.script
    write_record = test_clear_context.ClearContext.write_record
    tmux = test_clear_context.ClearContext.tmux
    wait_for = test_clear_context.ClearContext.wait_for
    received = test_clear_context.ClearContext.received
    clear = test_clear_context.ClearContext.clear

    def test_a_receipt_lost_after_enter_is_delivery_uncertain(self):
        on_keys(self, 'Enter', BLOCK_RECEIPTS.format('clear_receipts'))
        stderr = self.clear(success=False)
        self.assertIn('Clear delivery uncertain', stderr)
        self.wait_for(lambda: self.received() == b'/clear\r')
        self.assertEqual(self.received(), b'/clear\r')

    def test_a_record_unreadable_after_enter_is_delivery_uncertain(self):
        # The fake records Enter before freeze skips its processing. The
        # unreadable session can fail confirmation before that read completes.
        on_keys(self, 'Enter', 'touch "$INPUT_FIXTURE/freeze"\nrm -f "{0}"\nmkdir -p "{0}/blocked"\n'.format(self.record_path))
        stderr = self.clear(success=False)
        self.assertIn('Clear delivery uncertain', stderr)
        self.wait_for(lambda: self.received() == b'/clear\r')
        self.assertEqual(self.received(), b'/clear\r')

    def test_a_refusal_before_input_stays_definite(self):
        self.record.update(activity='busy', phase='working')
        self.write_record()
        stderr = self.clear(success=False)
        self.assertNotIn('uncertain', stderr.lower())
        self.assertEqual(self.received(), b'')


class TypedInterrupt(unittest.TestCase):
    setUp = test_session_actions.SessionActions.setUp
    tearDown = test_session_actions.SessionActions.tearDown
    script = test_session_actions.SessionActions.script
    write_record = test_session_actions.SessionActions.write_record
    tmux = test_session_actions.SessionActions.tmux
    wait_for = test_session_actions.SessionActions.wait_for
    received = test_session_actions.SessionActions.received
    request = test_session_actions.SessionActions.request
    identity = test_session_actions.SessionActions.identity
    working = test_session_actions.SessionActions.working

    def interrupt(self, ok):
        return self.request('interrupt', dict(self.identity(), expected_turn_started=self.record['turn_started']), ok=ok)

    def test_a_receipt_lost_after_escape_is_delivery_uncertain(self):
        self.working('kimi')
        on_keys(self, 'Escape', BLOCK_RECEIPTS.format('interrupt_receipts'))
        stderr = self.interrupt(ok=False)
        self.assertIn('Interrupt delivery uncertain', stderr)
        self.wait_for(lambda: self.received() == b'\x1b')

    def test_a_refusal_before_input_stays_definite(self):
        self.working('kimi')
        self.record.update(activity='idle', phase='idle')
        self.write_record()
        stderr = self.interrupt(ok=False)
        self.assertIn('This turn is no longer working', stderr)
        self.assertNotIn('uncertain', stderr.lower())
        self.assertEqual(self.received(), b'')


if __name__ == '__main__':
    unittest.main()
