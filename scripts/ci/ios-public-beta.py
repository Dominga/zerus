#!/usr/bin/env python3
"""Exact-build Public beta promotion and bounded, read-only health reporting.

Apple API writes require a previously uploaded per-build/action intent artifact.
A prior unresolved intent is read back, never posted again by another run.
"""
import argparse
import datetime
import importlib.util
import json
import os
from pathlib import Path
import re
import tempfile
import time
from typing import NamedTuple
import urllib.error
import urllib.parse
import urllib.request

SPEC = importlib.util.spec_from_file_location('asc_preflight', Path(__file__).with_name('ios-testflight-preflight.py'))
ASC = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ASC)
ORIGIN = 'https://api.appstoreconnect.apple.com'
REVIEW_STATES = {'WAITING_FOR_REVIEW', 'IN_REVIEW', 'REJECTED', 'APPROVED'}
EXTERNAL_STATES = {'PROCESSING', 'PROCESSING_EXCEPTION', 'MISSING_EXPORT_COMPLIANCE', 'READY_FOR_BETA_TESTING',
                   'IN_BETA_TESTING', 'EXPIRED', 'READY_FOR_BETA_SUBMISSION', 'IN_EXPORT_COMPLIANCE_REVIEW',
                   'WAITING_FOR_BETA_REVIEW', 'IN_BETA_REVIEW', 'BETA_REJECTED', 'BETA_APPROVED', 'NOT_APPLICABLE'}
APPROVED = {'READY_FOR_BETA_TESTING', 'IN_BETA_TESTING', 'BETA_APPROVED'}
PENDING_REVIEW = {'WAITING_FOR_BETA_REVIEW', 'IN_BETA_REVIEW'}
USABLE_EXTERNAL = APPROVED | PENDING_REVIEW | {'READY_FOR_BETA_SUBMISSION'}


class Refusal(ValueError):
    pass


def require(value, message):
    if not value:
        raise Refusal(message)


def identity(row, kind):
    require(isinstance(row, dict) and row.get('type') == kind, 'Unexpected ASC resource type')
    value = row.get('id', '')
    require(isinstance(value, str) and re.fullmatch(r'[A-Za-z0-9-]{1,128}', value), 'Invalid ASC resource identity')
    return value


def related(row, name, kind):
    value = row.get('relationships', {}).get(name, {}).get('data')
    return identity(value, kind)


class Request(NamedTuple):
    method: str
    path: str

    @property
    def scope(self):
        return self.method + ' ' + self.path


class Client:
    def __init__(self, key, readonly=True):
        self.key, self.readonly = key, readonly

    def request(self, method, path, query=None, body=None):
        require(method in ('GET', 'POST', 'PATCH') and re.fullmatch(r'/v1/[A-Za-z0-9/-]+', path), 'Invalid ASC operation')
        if method != 'GET':
            allowed = (method == 'POST' and (path in ('/v1/betaAppReviewSubmissions', '/v1/betaBuildLocalizations') or
                       re.fullmatch(r'/v1/betaGroups/[A-Za-z0-9-]+/relationships/builds', path))) or (
                       method == 'PATCH' and re.fullmatch(r'/v1/betaBuildLocalizations/[A-Za-z0-9-]+', path))
            require(not self.readonly and allowed, 'ASC write refused')
        if query:
            path += '?' + urllib.parse.urlencode(sorted(query.items()))
        operation = Request(method, path)
        try:
            bearer = ASC.token(self.key, os.environ['ASC_KEY_ID'], os.environ['ASC_ISSUER_ID'], int(time.time()), operation)
            request = urllib.request.Request(ORIGIN + path, method=method,
                data=json.dumps(body).encode() if body is not None else None,
                headers={'Authorization': 'Bearer ' + bearer, 'Content-Type': 'application/json'})
            with urllib.request.build_opener(ASC.NoRedirect).open(request, timeout=20) as response:
                raw = response.read(512 * 1024 + 1)
            require(len(raw) <= 512 * 1024, 'ASC response exceeds bounds')
            result = json.loads(raw) if raw else {}
            require(isinstance(result, dict), 'Malformed ASC response')
            return result
        except Refusal:
            raise
        except urllib.error.HTTPError as error:
            status = error.code; error.close()
            raise Refusal(f'ASC {method} refused (HTTP {status}); no response details logged') from None
        except Exception:
            raise Refusal(f'ASC {method} outcome unavailable; no response details logged') from None

    def collection(self, path, query):
        result = self.request('GET', path, query)
        rows = result.get('data')
        require(isinstance(rows, list) and len(rows) <= 200 and not result.get('links', {}).get('next'),
                'ASC collection is incomplete or exceeds 200 resources')
        return rows, result.get('included', [])


def age_hours(value, now):
    try:
        date = datetime.datetime.fromisoformat(value.replace('Z', '+00:00'))
        require(date.tzinfo is not None, 'Review submission has no timezone')
        age = (now - date).total_seconds() / 3600
        require(age >= -1, 'Review submission date is in the future')
        return max(0, age)
    except (ValueError, TypeError, AttributeError):
        raise Refusal('Review submission date is unknown') from None


class Monitor:
    def __init__(self, client, version, number=None):
        self.client, self.version, self.number = client, version, number
        self.members = []
        self.review = self.external = 'UNKNOWN'
        self.member = False
        self.app = self.group = self.build = None
        self.submitted = None
        self.notes = False
        self.localization = None

    def snapshot(self, wait_seconds=0):
        apps, _ = self.client.collection('/v1/apps', {'filter[bundleId]': 'app.zerus.mobile', 'limit': '2'})
        self.app = ASC.app_identity(apps)
        groups, _ = self.client.collection('/v1/betaGroups', {'filter[app]': self.app,
            'filter[name]': 'Public beta', 'include': 'app', 'limit': '2'})
        require(len(groups) == 1, 'Exact Public beta group is missing or ambiguous')
        group = groups[0]; self.group = identity(group, 'betaGroups')
        require(group.get('attributes', {}).get('name') == 'Public beta' and
                group.get('attributes', {}).get('isInternalGroup') is False and
                related(group, 'app', 'apps') == self.app, 'Public beta is not the exact external app group')
        query = {'filter[app]': self.app, 'filter[preReleaseVersion.version]': self.version,
                 'filter[preReleaseVersion.platform]': 'IOS', 'include': 'app,preReleaseVersion', 'limit': '2'}
        if self.number:
            query['filter[version]'] = self.number
        else:
            query.update(sort='-uploadedDate', limit='1')
        deadline = time.monotonic() + wait_seconds
        while True:
            builds, included = self.client.collection('/v1/builds', query)
            require(len(builds) <= 1, 'Exact iOS build is ambiguous')
            if builds:
                row = builds[0]; attrs = row.get('attributes', {})
                self.build = identity(row, 'builds')
                number = attrs.get('version', '')
                require(isinstance(number, str) and re.fullmatch('[1-9][0-9]{0,8}', number) and
                        (self.number is None or number == self.number), 'Wrong iOS build number')
                release = related(row, 'preReleaseVersion', 'preReleaseVersions')
                releases = [r for r in included if r.get('type') == 'preReleaseVersions' and r.get('id') == release]
                require(related(row, 'app', 'apps') == self.app and len(releases) == 1 and
                        releases[0].get('attributes', {}).get('version') == self.version and
                        releases[0].get('attributes', {}).get('platform') == 'IOS', 'Wrong iOS build app/version/platform')
                require(attrs.get('expired') is False, 'Exact iOS build is expired')
                state = attrs.get('processingState')
                require(state in ('PROCESSING', 'VALID'), 'Exact iOS build failed processing or is unknown')
                if state == 'VALID':
                    self.number = number
                    break
            require(time.monotonic() < deadline, 'Exact iOS build is missing or processing exceeded its bound')
            time.sleep(min(15, max(0, deadline - time.monotonic())))
        localizations, _ = self.client.collection('/v1/betaBuildLocalizations',
            {'filter[build]': self.build, 'filter[locale]': 'en-US', 'include': 'build', 'limit': '2'})
        require(len(localizations) <= 1, 'Ambiguous English What to Test localization')
        self.notes, self.localization = False, None
        if localizations:
            localization = localizations[0]
            self.localization = identity(localization, 'betaBuildLocalizations')
            require(related(localization, 'build', 'builds') == self.build and
                    localization.get('attributes', {}).get('locale') == 'en-US', 'Localization belongs to another build/locale')
            text = localization.get('attributes', {}).get('whatsNew')
            require(text is None or isinstance(text, str) and len(text) <= 10000, 'Malformed What to Test text')
            self.notes = isinstance(text, str) and bool(text.strip())
        self.members, _ = self.client.collection(f'/v1/betaGroups/{self.group}/builds', {'limit': '200'})
        member_ids = [identity(row, 'builds') for row in self.members]
        require(len(member_ids) == len(set(member_ids)), 'Duplicate group build identity')
        self.member = self.build in member_ids
        detail = self.client.request('GET', f'/v1/builds/{self.build}/buildBetaDetail', {'include': 'build'}).get('data')
        identity(detail, 'buildBetaDetails')
        require(related(detail, 'build', 'builds') == self.build, 'Beta detail belongs to another build')
        self.external = detail.get('attributes', {}).get('externalBuildState')
        require(self.external in EXTERNAL_STATES, 'Unknown external build state')
        reviews, _ = self.client.collection('/v1/betaAppReviewSubmissions',
            {'filter[build]': self.build, 'include': 'build', 'limit': '2'})
        require(len(reviews) <= 1, 'Ambiguous beta review submission')
        self.review, self.submitted = 'NOT_SUBMITTED', None
        if reviews:
            identity(reviews[0], 'betaAppReviewSubmissions')
            require(related(reviews[0], 'build', 'builds') == self.build, 'Review belongs to another build')
            self.review = reviews[0].get('attributes', {}).get('betaReviewState')
            self.submitted = reviews[0].get('attributes', {}).get('submittedDate')
            require(self.review in REVIEW_STATES, 'Unknown beta review state')
        require(self.review != 'REJECTED' and self.external != 'BETA_REJECTED', 'Beta review rejected this build')
        if self.review in ('WAITING_FOR_REVIEW', 'IN_REVIEW'):
            require(age_hours(self.submitted, datetime.datetime.now(datetime.timezone.utc)) <= 48,
                    'Beta review has been stuck for more than 48 hours')
        self.check_states()
        return self

    def check_states(self):
        # These are independent API reads; inconsistent observations grant no new write.
        require(self.external in USABLE_EXTERNAL, 'External build is not usable for beta testing')
        require(not (self.external in PENDING_REVIEW and self.review == 'NOT_SUBMITTED'),
                'External review is pending but its exact submission is missing; no new submission')
        require(self.review != 'APPROVED' or self.external in APPROVED,
                'Approved review disagrees with the external build state')

    def done(self, action):
        if action == 'notes':
            return self.notes
        return self.member if action == 'group' else self.review != 'NOT_SUBMITTED' or self.external in APPROVED

    def health(self):
        self.check_states()
        require(self.notes, 'Latest iOS Dev build has no English What to Test notes')
        require(self.member, 'Public beta is lagging behind the latest iOS Dev build')
        require(self.done('review'), 'Latest Public beta build has not been submitted for review')

    def payload(self, action):
        if action == 'notes':
            attrs = {'whatsNew': f'Zerus development build {self.version} ({self.number}). Try demo and report feedback.'}
            if self.localization:
                return 'PATCH', f'/v1/betaBuildLocalizations/{self.localization}', {
                    'data': {'type': 'betaBuildLocalizations', 'id': self.localization, 'attributes': attrs}}
            return 'POST', '/v1/betaBuildLocalizations', {'data': {'type': 'betaBuildLocalizations',
                'attributes': {**attrs, 'locale': 'en-US'},
                'relationships': {'build': {'data': {'type': 'builds', 'id': self.build}}}}}
        if action == 'group':
            return 'POST', f'/v1/betaGroups/{self.group}/relationships/builds', {'data': [{'type': 'builds', 'id': self.build}]}
        require(self.member, 'Review requires the exact build in Public beta first')
        require(self.notes, 'Review requires What to Test notes first')
        require(self.review == 'NOT_SUBMITTED' and self.external == 'READY_FOR_BETA_SUBMISSION',
                'New review requires an exact native submit-ready state')
        return 'POST', '/v1/betaAppReviewSubmissions', {'data': {'type': 'betaAppReviewSubmissions',
            'relationships': {'build': {'data': {'type': 'builds', 'id': self.build}}}}}


def artifact_name(version, number, action):
    return f'ios-public-beta-{version}-{number}-{action}'


def github_artifacts(name=None, artifact=None):
    path = f'/repos/ufna/zerus/actions/artifacts/{artifact}' if artifact else '/repos/ufna/zerus/actions/artifacts?'+urllib.parse.urlencode({'name': name, 'per_page': 2})
    request = urllib.request.Request('https://api.github.com' + path,
        headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'], 'Accept': 'application/vnd.github+json',
                 'X-GitHub-Api-Version': '2022-11-28'})
    try:
        with urllib.request.build_opener(ASC.NoRedirect).open(request, timeout=20) as response:
            raw = response.read(512 * 1024 + 1)
        require(len(raw) <= 512 * 1024, 'Intent lookup exceeds bounds')
        value = json.loads(raw)
        require(isinstance(value, dict), 'Malformed intent lookup')
        return value
    except Exception:
        raise Refusal('Durable intent lookup unavailable; no ASC write') from None


def plan(monitor, action, path):
    monitor.snapshot(wait_seconds=600)
    if monitor.done(action):
        return False
    if action == 'review':
        require(monitor.member, 'Review requires Public beta membership')
    name = artifact_name(monitor.version, monitor.number, action)
    history = github_artifacts(name=name)
    require(history.get('total_count') == 0 and history.get('artifacts') == [],
            'Prior write intent exists but is not observed at Apple; read-only recovery, no retry')
    value = {'schema': 1, 'action': action, 'version': monitor.version, 'number': monitor.number,
             'app': monitor.app, 'group': monitor.group, 'build': monitor.build,
             'run': os.environ['GITHUB_RUN_ID'], 'attempt': os.environ['GITHUB_RUN_ATTEMPT'],
             'source': os.environ['GITHUB_SHA'], 'created': time.time()}
    with path.open('x') as out:
        json.dump(value, out)
    return True


def apply(monitor, action, path, artifact):
    intent = json.loads(path.read_text())
    require(intent.get('schema') == 1 and intent.get('action') == action and
            intent.get('version') == monitor.version and intent.get('number') == monitor.number and
            intent.get('run') == os.environ['GITHUB_RUN_ID'] and intent.get('attempt') == os.environ['GITHUB_RUN_ATTEMPT'] and
            intent.get('source') == os.environ['GITHUB_SHA'] and 0 <= time.time() - intent.get('created', 0) <= 1200,
            'Write intent is stale or belongs to another original build/run')
    require(re.fullmatch('[1-9][0-9]*', artifact or ''), 'Known durable intent artifact required')
    record = github_artifacts(artifact=artifact)
    require(record.get('id') == int(artifact) and record.get('expired') is False and
            record.get('name') == artifact_name(monitor.version, monitor.number, action) and
            record.get('workflow_run', {}).get('id') == int(os.environ['GITHUB_RUN_ID']) and
            record.get('workflow_run', {}).get('head_sha') == os.environ['GITHUB_SHA'], 'Durable intent identity mismatch')
    monitor.snapshot()
    require((monitor.app, monitor.group, monitor.build) == (intent.get('app'), intent.get('group'), intent.get('build')),
            'Original app/group/build selection changed')
    if monitor.done(action):
        return
    method, endpoint, body = monitor.payload(action)
    try:
        with path.with_suffix('.attempted').open('x') as out:
            json.dump({'action': action, 'build': monitor.build, 'run': intent['run']}, out)
    except FileExistsError:
        raise Refusal('Original write already attempted; read-only recovery, no retry') from None
    # One mutation only. A timeout/error/invalid reply is resolved with GETs, never replayed.
    try:
        monitor.client.request(method, endpoint, body=body)
    except Refusal:
        pass
    end = time.monotonic() + 60
    while True:
        monitor.snapshot()
        if monitor.done(action):
            return
        require(time.monotonic() < end, 'Write outcome not observed; intent retained, no retry')
        time.sleep(min(10, max(0, end - time.monotonic())))


def summary(monitor, result):
    target = f'{monitor.version} ({monitor.number or "latest"})'
    lines = ['## iOS Public beta', '', f'Target: **{target}**', f'Result: **{result}**',
             f'Exact build in group: **{monitor.member}**', f'Beta review: **{monitor.review}**',
             f'External state: **{monitor.external}**', f'What to Test present: **{monitor.notes}**', '', 'Assigned build | Processing', '--- | ---']
    for row in monitor.members:
        attrs = row.get('attributes', {}); number = attrs.get('version', '')
        state = attrs.get('processingState', '')
        number = number if isinstance(number, str) and re.fullmatch('[1-9][0-9]{0,8}', number) else 'unknown'
        state = state if state in ('PROCESSING', 'VALID', 'FAILED', 'INVALID') else 'unknown'
        lines.append(f'{number} | {state}')
    if not monitor.members:
        lines.append('none/read unavailable | unknown')
    with Path(os.environ['GITHUB_STEP_SUMMARY']).open('a') as out:
        out.write('\n'.join(lines) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('plan', 'apply', 'status', 'check'))
    parser.add_argument('--action', choices=('notes', 'group', 'review'))
    parser.add_argument('--plan', type=Path)
    parser.add_argument('--artifact')
    args = parser.parse_args()
    monitor = None
    try:
        require(os.environ.get('GITHUB_REPOSITORY') == 'ufna/zerus' and os.environ.get('GITHUB_REF') == 'refs/heads/main',
                'Public beta automation requires trusted upstream main')
        version = (Path(__file__).resolve().parents[2] / 'mobile/ios/DEV_VERSION').read_text().strip()
        number = None if args.mode == 'status' else os.environ.get('BUILD_NUMBER', '')
        require(re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version) and
                (number is None or re.fullmatch('[1-9][0-9]{0,8}', number)), 'Exact iOS version/build required')
        require(args.mode in ('status', 'check') or args.action and args.plan, 'Exact action and intent path required')
        require(re.fullmatch('[A-Z0-9]{8,16}', os.environ.get('ASC_KEY_ID', '')) and
                re.fullmatch('[0-9a-f-]{36}', os.environ.get('ASC_ISSUER_ID', '')), 'Invalid ASC key identity')
        with tempfile.TemporaryDirectory(prefix='zerus-asc-beta-') as root:
            key = Path(root) / 'key.p8'
            with key.open('x') as out:
                os.chmod(key, 0o600); out.write(os.environ['ASC_KEY'])
            monitor = Monitor(Client(key, readonly=args.mode != 'apply'), version, number)
            if args.mode in ('status', 'check'):
                monitor.snapshot(); monitor.health()
            elif args.mode == 'plan':
                needed = plan(monitor, args.action, args.plan)
                with Path(os.environ['GITHUB_OUTPUT']).open('a') as out:
                    out.write('write=' + str(needed).lower() + '\nname=' + artifact_name(version, number, args.action) + '\n')
            else:
                apply(monitor, args.action, args.plan, args.artifact)
        if monitor:
            summary(monitor, 'read-only status' if args.mode in ('status', 'check') else 'observed')
        return 0
    except Exception as error:
        message = str(error) if isinstance(error, Refusal) else 'Public beta operation unavailable; no private details logged'
        if monitor:
            summary(monitor, message)
        print('::error::iOS Public beta: ' + message)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
