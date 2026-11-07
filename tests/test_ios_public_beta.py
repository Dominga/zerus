"""Synthetic ASC/GitHub only: exact builds, durable intents and no replay."""
import base64
import datetime
import importlib.util
import io
import itertools
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import urllib.error
import urllib.parse

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('ios_public_beta', ROOT/'scripts/ci/ios-public-beta.py')
beta = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(beta)


def row(kind, ident, **attrs):
    return dict(type=kind, id=ident, attributes=attrs)


def linked(value, name, kind, ident):
    value.setdefault('relationships', {})[name] = {'data': {'type': kind, 'id': ident}}
    return value


class FakeASC:
    def __init__(self, member=False, review=None, notes=True):
        self.member, self.review, self.notes = member, review, notes
        self.localization = notes
        self.external = {'WAITING_FOR_REVIEW': 'WAITING_FOR_BETA_REVIEW', 'IN_REVIEW': 'IN_BETA_REVIEW',
                         'APPROVED': 'BETA_APPROVED'}.get(review, 'READY_FOR_BETA_SUBMISSION')
        self.version, self.number, self.processing = '0.4.1', '2123', 'VALID'
        self.submitted = datetime.datetime.now(datetime.timezone.utc).isoformat()
        self.writes = []; self.accepted_error = False; self.lost = False

    def collection(self, path, query):
        if path == '/v1/apps':
            return [row('apps', '123', bundleId='app.zerus.mobile')], []
        if path == '/v1/betaGroups':
            return [linked(row('betaGroups', 'group-1', name='Public beta', isInternalGroup=False), 'app', 'apps', '123')], []
        if path == '/v1/builds':
            build = linked(row('builds', 'build-1', version=self.number, expired=False, processingState=self.processing), 'app', 'apps', '123')
            linked(build, 'preReleaseVersion', 'preReleaseVersions', 'release-1')
            return [build], [row('preReleaseVersions', 'release-1', version=self.version, platform='IOS')]
        if path == '/v1/betaBuildLocalizations':
            return ([linked(row('betaBuildLocalizations', 'notes-1', locale='en-US', whatsNew='Owner notes' if self.notes else ''), 'build', 'builds', 'build-1')] if self.localization else []), []
        if path == '/v1/betaGroups/group-1/builds':
            return [row('builds', 'build-1' if self.member else 'old-build', version=self.number if self.member else '849', processingState='VALID')], []
        if path == '/v1/betaAppReviewSubmissions':
            return ([linked(row('betaAppReviewSubmissions', 'review-1', betaReviewState=self.review, submittedDate=self.submitted), 'build', 'builds', 'build-1')] if self.review else []), []
        raise AssertionError(path)

    def request(self, method, path, query=None, body=None):
        if method == 'GET':
            assert path == '/v1/builds/build-1/buildBetaDetail'
            return {'data': linked(row('buildBetaDetails', 'detail-1', externalBuildState=self.external), 'build', 'builds', 'build-1')}
        self.writes.append((method, path, body))
        if not self.lost:
            if path.startswith('/v1/betaBuildLocalizations'): self.notes = self.localization = True
            elif path.endswith('/relationships/builds'): self.member = True
            else:
                assert path == '/v1/betaAppReviewSubmissions'
                self.review = 'WAITING_FOR_REVIEW'
                self.external = 'WAITING_FOR_BETA_REVIEW'
        if self.accepted_error or self.lost: raise beta.Refusal('ASC mutation outcome unavailable')
        return {}


class Promotion(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='ios-beta-test-')
        self.addCleanup(temporary.cleanup); self.root = Path(temporary.name)
        env = patch.dict(os.environ, {'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_SHA': 'a'*40})
        env.start(); self.addCleanup(env.stop)

    def monitor(self, **kwargs):
        return beta.Monitor(FakeASC(**kwargs), '0.4.1', '2123')

    def intent(self, monitor, action, filename=None):
        path = self.root/(filename or action+'.json')
        with patch.object(beta, 'github_artifacts', return_value={'total_count': 0, 'artifacts': []}):
            self.assertTrue(beta.plan(monitor, action, path))
        return path

    def artifact(self, action):
        return dict(id=42, expired=False, name=beta.artifact_name('0.4.1', '2123', action), workflow_run=dict(id=123, head_sha='a'*40))

    def apply(self, monitor, action, path, artifact=None):
        with patch.object(beta, 'github_artifacts', return_value=artifact or self.artifact(action)), \
             patch.object(beta.time, 'monotonic', side_effect=itertools.count(0, 100)), patch.object(beta.time, 'sleep'):
            beta.apply(monitor, action, path, '42')

    def test_exact_group_and_review_writes_happen_once(self):
        m = self.monitor()
        for action in ('group', 'review'): self.apply(m, action, self.intent(m, action))
        self.assertEqual([w[:2] for w in m.client.writes], [('POST', '/v1/betaGroups/group-1/relationships/builds'), ('POST', '/v1/betaAppReviewSubmissions')])
        self.assertEqual(m.client.writes[0][2], {'data': [{'type': 'builds', 'id': 'build-1'}]})
        self.assertEqual(m.client.writes[1][2]['data']['relationships']['build']['data']['id'], 'build-1')
        for action in ('group', 'review'):
            with patch.object(beta, 'github_artifacts') as lookup:
                self.assertFalse(beta.plan(m, action, self.root/'unused')); lookup.assert_not_called()
        self.assertEqual(len(m.client.writes), 2)

    def test_each_unknown_mutation_is_read_back_without_replay(self):
        for action in ('notes', 'group', 'review'):
            with self.subTest(action=action):
                m = self.monitor(member=action == 'review', notes=action != 'notes'); m.client.accepted_error = True
                self.apply(m, action, self.intent(m, action))
                self.assertEqual(len(m.client.writes), 1); self.assertTrue(m.done(action))

    def test_unobserved_write_and_next_run_fail_without_retry(self):
        m = self.monitor(); m.client.lost = True; path = self.intent(m, 'group')
        with self.assertRaisesRegex(beta.Refusal, 'outcome not observed'): self.apply(m, 'group', path)
        with patch.object(beta, 'github_artifacts', return_value={'total_count': 1, 'artifacts': [self.artifact('group')]}), self.assertRaisesRegex(beta.Refusal, 'Prior write intent'):
            beta.plan(m, 'group', self.root/'next-run')
        self.assertEqual(len(m.client.writes), 1); self.assertFalse((self.root/'next-run').exists())
        with self.assertRaisesRegex(beta.Refusal, 'already attempted'):
            self.apply(m, 'group', path)
        self.assertEqual(len(m.client.writes), 1)

    def test_terminal_state_appearing_after_plan_prevents_the_write(self):
        m = self.monitor(); path = self.intent(m, 'group'); m.client.member = True
        self.apply(m, 'group', path)
        self.assertEqual(m.client.writes, [])

    def test_lost_review_and_notes_patch_also_keep_the_intent(self):
        for action in ('notes', 'review'):
            m = self.monitor(member=True, notes=action != 'notes')
            if action == 'notes': m.client.localization = True
            m.client.lost = True; path = self.intent(m, action)
            with self.assertRaisesRegex(beta.Refusal, 'outcome not observed'): self.apply(m, action, path)
            self.assertEqual(len(m.client.writes), 1)
            if action == 'notes': self.assertEqual(m.client.writes[0][:2], ('PATCH', '/v1/betaBuildLocalizations/notes-1'))
            with patch.object(beta, 'github_artifacts', return_value={'total_count': 1, 'artifacts': [self.artifact(action)]}), self.assertRaisesRegex(beta.Refusal, 'Prior write intent'):
                beta.plan(m, action, self.root/'no-retry')
            self.assertEqual(len(m.client.writes), 1)

    def test_expired_foreign_unbound_or_missing_durable_artifact_refuses(self):
        m = self.monitor(); path = self.intent(m, 'group')
        for change in ({'expired': True}, {'id': 41}, {'name': 'another-build'}, {'workflow_run': {'id': 124, 'head_sha': 'a'*40}}, {'workflow_run': {'id': 123, 'head_sha': 'b'*40}}):
            with self.subTest(change=change), self.assertRaisesRegex(beta.Refusal, 'Durable intent'):
                self.apply(m, 'group', path, {**self.artifact('group'), **change})
        with self.assertRaisesRegex(beta.Refusal, 'artifact required'): beta.apply(m, 'group', path, None)
        self.assertEqual(m.client.writes, [])

    def test_stale_intent_and_changed_original_build_refuse(self):
        for key, value in [('source', 'b'*40), ('number', '2124'), ('created', 0), ('attempt', '2'), ('build', 'another-build')]:
            with self.subTest(key=key):
                m = self.monitor(); path = self.intent(m, 'group', key+'.json'); data = json.loads(path.read_text()); data[key] = value; path.write_text(json.dumps(data))
                with self.assertRaises(beta.Refusal): self.apply(m, 'group', path)
                self.assertEqual(m.client.writes, [])

    def test_rejected_and_stuck_review_fail_without_resubmission(self):
        for state, hours, message in [('REJECTED', 0, 'rejected'), ('WAITING_FOR_REVIEW', 49, 'stuck'), ('IN_REVIEW', 49, 'stuck')]:
            with self.subTest(state=state):
                m = self.monitor(member=True, review=state)
                m.client.submitted = (datetime.datetime.now(datetime.timezone.utc)-datetime.timedelta(hours=hours)).isoformat()
                with self.assertRaisesRegex(beta.Refusal, message): m.snapshot()
                self.assertEqual(m.client.writes, [])

    def test_waiting_in_review_and_approved_are_normal(self):
        for state in ('WAITING_FOR_REVIEW', 'IN_REVIEW', 'APPROVED'):
            with self.subTest(state=state):
                m = self.monitor(member=True, review=state); m.snapshot(); m.health()
                with patch.object(beta, 'github_artifacts') as lookup:
                    self.assertFalse(beta.plan(m, 'review', self.root/'unused')); lookup.assert_not_called()
                self.assertEqual(m.client.writes, [])

    def test_pending_external_review_with_missing_submission_refuses_without_writes(self):
        for state in beta.PENDING_REVIEW:
            with self.subTest(external=state):
                m = self.monitor(member=True); m.client.external = state
                with patch.object(beta, 'github_artifacts') as history, self.assertRaisesRegex(beta.Refusal, 'exact submission is missing'):
                    beta.plan(m, 'review', self.root/'must-not-create')
                history.assert_not_called(); self.assertFalse((self.root/'must-not-create').exists())
                self.assertEqual(m.client.writes, [])

    def test_approved_review_cannot_hide_unusable_or_contradictory_external_state(self):
        for state in beta.EXTERNAL_STATES - beta.APPROVED:
            with self.subTest(external=state):
                m = self.monitor(member=True, review='APPROVED'); m.client.external = state
                with self.assertRaises(beta.Refusal): m.snapshot()
                with self.assertRaises(beta.Refusal): m.health()
                with self.assertRaises(beta.Refusal): beta.plan(m, 'review', self.root/'must-not-create')
                self.assertEqual(m.client.writes, [])

    def test_unusable_external_state_refuses_with_any_review_state(self):
        for state in beta.EXTERNAL_STATES - beta.USABLE_EXTERNAL:
            for review in (None, 'WAITING_FOR_REVIEW', 'IN_REVIEW', 'APPROVED'):
                with self.subTest(external=state, review=review):
                    m = self.monitor(member=True, review=review); m.client.external = state
                    with self.assertRaises(beta.Refusal): m.snapshot()
                    with self.assertRaises(beta.Refusal): m.health()
                    self.assertEqual(m.client.writes, [])

    def test_review_payload_itself_requires_original_submit_ready_state(self):
        m = self.monitor(member=True); m.snapshot()
        for state in beta.EXTERNAL_STATES - {'READY_FOR_BETA_SUBMISSION'}:
            with self.subTest(external=state):
                m.external = state
                with self.assertRaisesRegex(beta.Refusal, 'submit-ready'): m.payload('review')
        self.assertEqual(m.client.writes, [])

    def test_native_approved_state_needs_no_new_submission(self):
        for state in beta.APPROVED:
            m = self.monitor(member=True); m.client.external = state; m.snapshot(); m.health()
            self.assertTrue(m.done('review')); self.assertEqual(m.client.writes, [])

    def test_old_group_build_cannot_hide_lag(self):
        m = self.monitor(review='APPROVED'); m.snapshot(); self.assertEqual(m.members[0]['attributes']['version'], '849')
        with self.assertRaisesRegex(beta.Refusal, 'lagging'): m.health()

    def test_processing_bound_and_fresh_read(self):
        m = self.monitor(); m.client.processing = 'PROCESSING'
        with patch.object(beta.time, 'monotonic', side_effect=[0, 601]), self.assertRaisesRegex(beta.Refusal, 'processing exceeded'):
            m.snapshot(wait_seconds=600)
        m = self.monitor(); original = m.client.collection; seen = []
        def read(path, query):
            if path == '/v1/builds': seen.append(path); m.client.processing = 'PROCESSING' if len(seen) == 1 else 'VALID'
            return original(path, query)
        with patch.object(m.client, 'collection', side_effect=read), patch.object(beta.time, 'sleep'), patch.object(beta.time, 'monotonic', return_value=0): m.snapshot(wait_seconds=600)
        self.assertEqual(len(seen), 2)

    def test_wrong_version_number_platform_and_group_refuse(self):
        for path, change in [('/v1/builds', lambda rows, inc: rows[0]['attributes'].update(version='2124')),
                ('/v1/builds', lambda rows, inc: inc[0]['attributes'].update(version='0.5.0')),
                ('/v1/builds', lambda rows, inc: inc[0]['attributes'].update(platform='MAC_OS')),
                ('/v1/betaGroups', lambda rows, inc: rows[0]['attributes'].update(isInternalGroup=True)),
                ('/v1/betaGroups', lambda rows, inc: rows[0]['relationships']['app']['data'].update(id='999'))]:
            m = self.monitor(); original = m.client.collection
            def changed(p, q):
                rows, inc = original(p, q)
                if p == path: change(rows, inc)
                return rows, inc
            with patch.object(m.client, 'collection', side_effect=changed), self.assertRaises(beta.Refusal): m.snapshot()
            self.assertEqual(m.client.writes, [])

    def test_what_to_test_post_and_patch_routes_preserve_existing_notes(self):
        for exists, method, endpoint in [(False, 'POST', '/v1/betaBuildLocalizations'), (True, 'PATCH', '/v1/betaBuildLocalizations/notes-1')]:
            m = self.monitor(notes=False); m.client.localization = exists
            self.apply(m, 'notes', self.intent(m, 'notes', str(exists)+'.json'))
            self.assertEqual(m.client.writes[0][:2], (method, endpoint))
            self.assertIn('2123', m.client.writes[0][2]['data']['attributes']['whatsNew'])
        m = self.monitor()
        with patch.object(beta, 'github_artifacts') as lookup:
            self.assertFalse(beta.plan(m, 'notes', self.root/'unused')); lookup.assert_not_called()

    def test_summary_lists_group_and_review_without_arbitrary_text(self):
        m = self.monitor(member=True, review='WAITING_FOR_REVIEW'); m.snapshot()
        m.members.append(row('builds', 'untrusted', version='private-bearer', processingState='private-key'))
        summary = self.root/'summary'
        with patch.dict(os.environ, {'GITHUB_STEP_SUMMARY': str(summary)}): beta.summary(m, 'read-only status')
        text = summary.read_text()
        for expected in ('2123', 'WAITING_FOR_REVIEW', 'VALID', 'unknown'): self.assertIn(expected, text)
        self.assertNotIn('private-', text); self.assertNotIn('Owner notes', text)


class RequestsAndWorkflow(unittest.TestCase):
    def test_readonly_refuses_all_mutations_before_signing(self):
        for method, path in [('POST', '/v1/betaAppReviewSubmissions'), ('POST', '/v1/betaBuildLocalizations'), ('PATCH', '/v1/betaBuildLocalizations/notes-1'), ('POST', '/v1/betaGroups/group-1/relationships/builds')]:
            with patch.dict(os.environ, {'ASC_KEY_ID': 'ABCDEFGH12', 'ASC_ISSUER_ID': '11111111-1111-4111-8111-111111111111'}), patch.object(beta.ASC, 'token', return_value='synthetic-bearer') as sign, patch.object(beta.urllib.request, 'build_opener', return_value=SimpleNamespace(open=lambda *a, **k: io.BytesIO(b'{}'))) as opener, self.assertRaises(beta.Refusal):
                beta.Client(Path('/key')).request(method, path, body={})
            sign.assert_not_called(); opener.assert_not_called()

    def test_fresh_method_scopes_never_reuse_get_token_on_patch_or_post(self):
        methods = []; bearers = []; scopes = []; actual_paths = []
        def opened(request, timeout):
            self.assertEqual(timeout, 20); bearer = request.get_header('Authorization').removeprefix('Bearer ')
            claims = json.loads(base64.urlsafe_b64decode(bearer.split('.')[1]+'==')); url = urllib.parse.urlsplit(request.full_url)
            scopes.append(claims['scope']); actual_paths.append(request.get_method()+' '+url.path+('?' + url.query if url.query else ''))
            methods.append(request.get_method()); bearers.append(bearer); return io.BytesIO(b'{}')
        with patch.dict(os.environ, {'ASC_KEY_ID': 'ABCDEFGH12', 'ASC_ISSUER_ID': '11111111-1111-4111-8111-111111111111'}), \
             patch.object(beta.ASC.subprocess, 'check_output', return_value=b'\x30\x06\x02\x01\x01\x02\x01\x02') as sign, \
             patch.object(beta.urllib.request, 'build_opener', return_value=SimpleNamespace(open=opened)):
            client = beta.Client(Path('/key'), readonly=False)
            client.request('GET', '/v1/builds', {'filter[app]': '123', 'limit': '2'})
            client.request('POST', '/v1/betaBuildLocalizations', body={}); client.request('PATCH', '/v1/betaBuildLocalizations/notes-1', body={})
        self.assertEqual(methods, ['GET', 'POST', 'PATCH']); self.assertEqual(sign.call_count, 3); self.assertEqual(len(set(bearers)), 3)
        self.assertEqual(scopes, [[path] for path in actual_paths])

    def test_http_failure_and_redirect_do_not_echo_secrets_or_retry(self):
        def opened(request, timeout): raise urllib.error.HTTPError(request.full_url, 405, 'private-key', {}, io.BytesIO(b'private-bearer'))
        with patch.dict(os.environ, {'ASC_KEY_ID': 'ABCDEFGH12', 'ASC_ISSUER_ID': '11111111-1111-4111-8111-111111111111'}), patch.object(beta.ASC, 'token', return_value='private-bearer'), patch.object(beta.urllib.request, 'build_opener', return_value=SimpleNamespace(open=opened)) as opener, self.assertRaisesRegex(beta.Refusal, 'HTTP 405') as error:
            beta.Client(Path('/key'), readonly=False).request('PATCH', '/v1/betaBuildLocalizations/notes-1', body={})
        self.assertNotIn('private', str(error.exception)); self.assertEqual(opener.call_count, 1)
        with self.assertRaises(ValueError): beta.ASC.NoRedirect().redirect_request(None, None, 302, 'redirect', {}, 'https://foreign.example')

    def test_incomplete_collections_refuse(self):
        client = beta.Client(Path('/key'))
        for result in ({'data': [], 'links': {'next': 'https://foreign.example'}}, {'data': [{}]*201}):
            with patch.object(client, 'request', return_value=result), self.assertRaises(beta.Refusal): client.collection('/v1/builds', {'limit': '200'})

    def test_non_main_refuses_before_keys_and_network(self):
        with patch.object(sys, 'argv', ['ios-public-beta.py', 'status']), patch.dict(os.environ, {'GITHUB_REPOSITORY': 'ufna/zerus', 'GITHUB_REF': 'refs/pull/1/merge', 'ASC_KEY_ID': 'ABCDEFGH12', 'ASC_ISSUER_ID': '11111111-1111-4111-8111-111111111111', 'ASC_KEY': 'synthetic-key'}, clear=True), patch.object(beta, 'Client') as client, patch.object(beta.tempfile, 'TemporaryDirectory') as key, patch('sys.stdout', io.StringIO()):
            self.assertEqual(beta.main(), 1)
        client.assert_not_called(); key.assert_not_called()

    def test_real_entry_writes_outputs_summary_and_cleans_the_private_key(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); output = root/'output'; summary = root/'summary'
            env = {'GITHUB_REPOSITORY': 'ufna/zerus', 'GITHUB_REF': 'refs/heads/main',
                   'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_SHA': 'a'*40,
                   'BUILD_NUMBER': '2123', 'ASC_KEY_ID': 'ABCDEFGH12',
                   'ASC_ISSUER_ID': '11111111-1111-4111-8111-111111111111',
                   'ASC_KEY': 'synthetic-key-never-signed', 'GITHUB_OUTPUT': str(output),
                   'GITHUB_STEP_SUMMARY': str(summary)}
            observed = []
            version = (ROOT/'mobile/ios/DEV_VERSION').read_text().strip()
            def client(key, readonly):
                observed.append(key); self.assertTrue(readonly)
                self.assertEqual(key.read_text(), 'synthetic-key-never-signed')
                self.assertEqual(key.stat().st_mode & 0o777, 0o600)
                native = FakeASC(); native.version = version; return native
            with patch.dict(os.environ, env, clear=True), patch.object(sys, 'argv', ['ios-public-beta.py', 'plan', '--action', 'group', '--plan', str(root/'plan.json')]), patch.object(beta, 'Client', side_effect=client), patch.object(beta, 'github_artifacts', return_value={'total_count': 0, 'artifacts': []}):
                self.assertEqual(beta.main(), 0)
            self.assertEqual(output.read_text(), f'write=true\nname=ios-public-beta-{version}-2123-group\n')
            self.assertIn('2123', summary.read_text()); self.assertFalse(observed[0].exists())
            self.assertNotIn('synthetic-key', (root/'plan.json').read_text())

    def test_readonly_status_lag_really_returns_failure_and_summary(self):
        with tempfile.TemporaryDirectory() as directory:
            summary = Path(directory)/'summary'
            native = FakeASC(); native.version = (ROOT/'mobile/ios/DEV_VERSION').read_text().strip()
            env = {'GITHUB_REPOSITORY': 'ufna/zerus', 'GITHUB_REF': 'refs/heads/main',
                   'ASC_KEY_ID': 'ABCDEFGH12', 'ASC_ISSUER_ID': '11111111-1111-4111-8111-111111111111',
                   'ASC_KEY': 'synthetic-key', 'GITHUB_STEP_SUMMARY': str(summary)}
            with patch.dict(os.environ, env, clear=True), patch.object(sys, 'argv', ['ios-public-beta.py', 'status']), patch.object(beta, 'Client', return_value=native) as client, patch('sys.stdout', io.StringIO()) as output:
                self.assertEqual(beta.main(), 1)
                self.assertIn('::error::', output.getvalue())
            self.assertTrue(client.call_args.kwargs['readonly'])
            self.assertIn('lagging', summary.read_text()); self.assertIn('849', summary.read_text())
            self.assertEqual(native.writes, [])

    def test_workflow_conditions_refuse_prs_forks_and_unsuccessful_uploads(self):
        from test_ci_security import job, field
        text = (ROOT/'.github/workflows/ios-testflight.yml').read_text()
        for job_id in ('public-beta', 'public-beta-status'):
            expression = field(job(text, job_id), 'if').replace('&&', ' and ').replace('||', ' or ').replace('!cancelled()', 'not cancelled()').replace('false', 'False')
            def runs(ref='refs/heads/main', repository='ufna/zerus', private=False, event='schedule', channel=None, result='success', enabled='true'):
                selected = ('dev' if job_id == 'public-beta' else '') if channel is None else channel
                return bool(eval(expression, {'__builtins__': {}}, {'cancelled': lambda: False,
                    'github': SimpleNamespace(ref=ref, repository=repository, event_name=event,
                        event=SimpleNamespace(repository=SimpleNamespace(private=private))),
                    'inputs': SimpleNamespace(channel=selected), 'vars': SimpleNamespace(IOS_TESTFLIGHT_ENABLED=enabled),
                    'needs': SimpleNamespace(upload=SimpleNamespace(result=result))}))
            self.assertTrue(runs())
            for change in ({'ref': 'refs/pull/1/merge'}, {'ref': 'refs/heads/topic'}, {'repository': 'fork/zerus'}, {'private': True}):
                self.assertFalse(runs(**change))
            if job_id == 'public-beta':
                self.assertFalse(runs(result='failure')); self.assertFalse(runs(channel='stable'))
                self.assertFalse(runs(event='pull_request')); self.assertFalse(runs(event='pull_request_target'))
                self.assertFalse(runs(event='push')); self.assertFalse(runs(enabled='false'))
            else:
                self.assertFalse(runs(event='pull_request')); self.assertFalse(runs(channel='dev')); self.assertFalse(runs(enabled='false'))

    def test_workflow_secret_boundaries_original_build_and_independent_status(self):
        from test_ci_security import job, field, steps
        text = (ROOT/'.github/workflows/ios-testflight.yml').read_text(); upload = job(text, 'upload'); promote = job(text, 'public-beta'); status = job(text, 'public-beta-status')
        self.assertIn('number: ${{ steps.build.outputs.number }}', upload); self.assertEqual(field(promote, 'needs'), 'upload')
        self.assertIn('BUILD_NUMBER: ${{ needs.upload.outputs.number }}', promote); self.assertEqual(field(promote, 'continue-on-error'), 'true')
        self.assertNotIn('continue-on-error', status); self.assertNotIn('needs:', status); self.assertIn("inputs.channel == ''", status)
        self.assertIn("cron: '47 */6 * * *'", text); self.assertNotIn(' apply ', status); self.assertNotIn(' plan ', status)
        for body in (promote, status):
            self.assertEqual(field(body, 'environment'), 'ios-testflight'); self.assertIn("github.ref == 'refs/heads/main'", body)
            self.assertIn("github.repository == 'ufna/zerus'", body); self.assertIn('persist-credentials: false', body); self.assertNotIn('IOS_DEVELOPMENT_P12', body)
        listed = steps(promote)
        for action in ('notes', 'group', 'review'):
            prepared = next(s for s in listed if f'id: plan-{action}\n' in s); reserved = next(s for s in listed if f'id: reserve-{action}\n' in s)
            applied = next(s for s in listed if f'apply --action {action}' in s)
            self.assertLess(listed.index(prepared), listed.index(reserved)); self.assertLess(listed.index(reserved), listed.index(applied))
            self.assertIn('retention-days: 90', reserved); self.assertNotIn('secrets.', reserved)
            self.assertIn(f'steps.reserve-{action}.outputs.artifact-id', applied); self.assertIn(f"if: steps.plan-{action}.outputs.write == 'true'", applied)


if __name__ == '__main__': unittest.main()
