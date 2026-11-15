"""Linux read-volume controls for unindexed discovery and indexed fallback.

Use /proc on our unreaped child, never the user's state or process inventory.
The companion scale benchmark uses the same final I/O and wait4 accounting.
"""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
import uuid

HGS = Path(os.environ.get('HGS_TEST_BIN', Path(__file__).resolve().parents[1] / 'target/debug/hgs')).resolve()


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux child I/O accounting')
class SearchScale(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix='hgs-search-scale-')
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.state = self.root / 'state'
        (self.state / 'archives').mkdir(parents=True)
        self.env = {k:v for k,v in os.environ.items() if not k.startswith('HGS_') and k not in ('TMUX', 'TMUX_PANE')}
        self.env.update(HOME=str(self.root), HGS_STATE_DIR=str(self.state),
                        HGS_CONFIG_DIR=str(self.root / 'config'), HGS_SELF='example', HGS_PEERS='')
        jobs = {str(n): dict(status='unknown', command='printf synthetic', output='x' * (16 << 10)) for n in range(8)}
        for n in range(640):
            name = f'codex/example/session-{n}'
            value = dict(version=1, name=name, agent='codex', run_id=f'run-{n}',
                         conversation_id=f'conversation-{n}', updated=n, last_event_at=n,
                         last_message='A searchable synthetic result', shell_jobs=jobs)
            if n < 192:
                path = self.state / (hashlib.sha256(name.encode()).hexdigest() + '.json')
            else:
                ident = str(uuid.UUID(int=n+1))
                value.update(archive_id=ident, archived_at=n, state='archived')
                path = self.state / 'archives' / (ident + '.json')
            path.write_text(json.dumps(value), encoding='utf-8')

    def query(self, indexed=False, query='synthetic'):
        with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
            process = subprocess.Popen([str(HGS), '__state', 'search', '--query', query, '--limit', '20'] +
                                       (['--indexed'] if indexed else []), env=self.env,
                                       stdin=subprocess.DEVNULL, stdout=out, stderr=err)
            until = time.monotonic() + 30
            try:
                while not os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT):
                    if time.monotonic() >= until:
                        raise TimeoutError('private search exceeded 30 seconds')
                    time.sleep(.005)
                io = dict(line.split(':') for line in Path(f'/proc/{process.pid}/io').read_text().splitlines())
                _, status, usage = os.wait4(process.pid, 0)
                process.returncode = os.waitstatus_to_exitcode(status)
            finally:
                if process.returncode is None:
                    process.send_signal(signal.SIGKILL)
                    process.wait()
            out.seek(0); err.seek(0)
            self.assertEqual(process.returncode, 0, err.read().decode())
            value = json.load(out)
        return value, int(io['rchar']), usage.ru_maxrss

    def assert_bounded(self, indexed):
        value, read, rss = self.query(indexed)
        self.assertLess(read, 12 << 20, f'discovery/fallback read {read} bytes before a bounded result')
        self.assertLess(rss, 96 << 10, f'discovery retained {rss} KiB of shell-job histories')
        result = value['fallback'] if indexed else value
        self.assertTrue(result['truncated'], 'an incomplete inventory must not claim complete search coverage')
        self.assertTrue(result['results'], 'bounded discovery must still search the records it admitted')

    def test_ordinary_search_bounds_discovery_before_loading_saved_histories(self):
        self.assert_bounded(False)

    def test_indexed_search_has_a_bounded_fallback_when_coverage_is_incomplete(self):
        self.assert_bounded(True)

    def test_discovery_gap_is_reported_even_when_nothing_matches(self):
        value, _, _ = self.query(query='definitely-absent-in-private-fixture')
        self.assertEqual(value['results'], [])
        self.assertTrue(value['truncated'], 'no matches in a partial inventory is incomplete coverage')
        self.assertFalse(value['discovery']['complete'])


if __name__ == '__main__':
    unittest.main()
