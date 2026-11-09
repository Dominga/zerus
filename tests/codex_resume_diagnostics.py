"""Bounded, read-only evidence for a native Codex resume in a private fixture."""
from contextlib import closing
import json
from pathlib import Path
import sqlite3


LIMIT = 64 * 1024
METADATA_LIMIT = 1024 * 1024


def sqlite_log_tail(home):
    """Recent native SQLite logs, without creating a missing database or retaining its handle."""
    try:
        with closing(sqlite3.connect((home / 'logs_2.sqlite').resolve().as_uri() + '?mode=ro',
                                     uri=True, timeout=0.2)) as database:
            rows = database.execute("""
                SELECT level, target, body, process FROM (
                    SELECT id, substr(level, 1, 16) AS level, substr(target, 1, 256) AS target,
                           substr(feedback_log_body, 1, 2048) AS body,
                           substr(process_uuid, 1, 128) AS process
                    FROM logs ORDER BY id DESC LIMIT 64
                ) WHERE level IN ('WARN', 'ERROR') OR target LIKE '%rollout%'
                  OR target LIKE '%session_resume%' ORDER BY id DESC
            """).fetchall()
        kept = []
        for row in rows:
            if len(json.dumps([*kept, row], ensure_ascii=False).encode()) > LIMIT:
                break
            kept.append(row)
        return dict(rows=kept, truncated=len(kept) < len(rows), window=64)
    except sqlite3.Error as error:
        return dict(unavailable=type(error).__name__)


def tail(path, limit=LIMIT):
    try:
        with path.open('rb') as stream:
            stream.seek(0, 2)
            stream.seek(max(0, stream.tell() - limit))
            return stream.read(limit).decode('utf-8', 'replace').encode('utf-8')[-limit:].decode('utf-8', 'ignore')
    except OSError as error:
        return f'(unavailable: {type(error).__name__})'


class ResumeDiagnostics:
    def __init__(self, name, original):
        self.name, self.original = name, original
        self.launch = None
        self.evidence = {'original': self.record(original)}

    @staticmethod
    def record(value):
        keys = ('run_id', 'conversation_id', 'expected_id', 'pid', 'process_start', 'supervisor',
                'base', 'transcript', 'startup_kind', 'error')
        text = json.dumps({key: value.get(key) for key in keys}, ensure_ascii=False)
        return json.loads(text) if len(text.encode()) <= LIMIT else {'unavailable': 'record over limit'}

    def sample(self, record, query):
        # Neither an older binding nor another conversation is this resume.
        if (not self.original.get('conversation_id') or not record.get('run_id') or record.get('run_id') == self.original.get('run_id')
                or record.get('expected_id') != self.original.get('conversation_id')):
            return
        self.evidence['resumed'] = self.record(record)
        if self.launch is None:
            reply = query({'op': 'list'})
            if not isinstance(reply, dict):
                return
            sessions = reply.get('sessions', [])
            if not isinstance(sessions, list) or len(sessions) > 16:
                return
            found = [row for row in sessions if row.get('name') == self.name]
            if len(found) != 1:
                return
            row = found[0]
            launch = row.get('launch_id')
            if not isinstance(launch, str) or not 0 < len(launch) <= 256:
                return
            self.launch = {key: row.get(key) for key in ('id', 'name', 'launch_id')}
            self.evidence['host_launch'] = self.launch
        reply = query({'op': 'launch_status', 'launch_id': self.launch['launch_id']})
        if (isinstance(reply, dict) and reply.get('session') == self.launch['id']
                and reply.get('name') == self.name):
            text = json.dumps(reply, ensure_ascii=False)
            if len(text.encode()) <= 8192:
                self.evidence['original_launch_status'] = reply

    def finish(self, home):
        # ConPTY merges stdout/stderr. launch_status.output is the original
        # drained terminal tail, never a replay or another child's output.
        self.evidence['native_log_tail'] = tail(home / 'log/codex-tui.log')
        self.evidence['native_sqlite_log_tail'] = sqlite_log_tail(home)
        self.evidence['recorded_argv_basis'] = 'HGS saved native base argv, not an OS argv observation'
        self.evidence['session_files_basis'] = 'retained source metadata, not a process file-read audit'
        source = self.original.get('transcript')
        if source:
            path = Path(source)
            try:
                if not path.resolve().is_relative_to(home.resolve()):
                    raise ValueError('source outside fixture home')
                with path.open('rb') as stream:
                    first = stream.readline(METADATA_LIMIT + 1)
                if len(first) > METADATA_LIMIT:
                    raise ValueError('metadata over limit')
                row = json.loads(first)
                data = row.get('payload', {})
                self.evidence['original_session_file'] = dict(path=str(path), bytes=path.stat().st_size,
                    type=row.get('type'), metadata={key: data.get(key) for key in
                    ('id', 'cwd', 'cli_version', 'model_provider', 'source', 'originator')})
            except (OSError, ValueError) as error:
                self.evidence['original_session_file'] = {'unavailable': type(error).__name__}
        result = json.dumps(self.evidence, ensure_ascii=False)
        return result if len(result.encode()) <= 4 * LIMIT else json.dumps({'unavailable': 'diagnostic over limit'})
