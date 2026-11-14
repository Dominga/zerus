#!/usr/bin/env python3
"""Fixed, keyless Kimi contracts on disposable hosted runners; never an installer.

Downloads precede native execution. The only model/platform services during a
case are the reviewed fixtures' loopback services. This is not an OS egress
enclosure. A timeout fails and keeps exact recorded roots; terminating the
unreaped suite child does not establish detached descendants' quiescence.
"""
import argparse
import ast
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
# Historical data validation only; native execution always uses stage_latest().
HISTORICAL_VERSIONS = ('2.1.1', '2.2.0')
METHODS = (
    # a4mub.1.1: genuine native draft refusal, exact spent-UUID proof, no keys
    # or model request, and a read-only original-only v2 recovery lookup.
    'test_kimi_native.NativeKimi.test_native_refusal_has_an_opted_in_terminal_outcome',
    'test_kimi_native.NativeKimi.test_declined_initial_prompt_is_cancelled_and_inspection_is_bounded',
    'test_kimi_native.NativeKimi.test_launch_prompt_waits_for_trust_and_is_not_replayed_on_resume',
    'test_kimi_native.NativeKimi.test_exact_input_reaches_the_model_once',
    'test_kimi_native.NativeKimi.test_background_task_completion_settles_its_own_main_turn',
    'test_kimi_native.NativeKimi.test_busy_attachment_send_is_typed_once_and_delivered_once',
    'test_kimi_native.NativeKimi.test_clear_input_discards_a_residue_and_kimi_keeps_running',
    'test_kimi_native.NativeKimi.test_hint_reaches_model_through_real_hgs_hook',
    # #2195 (zerus-cdhjm.3): private cache expiry compacts before one delivery,
    # retaining run/CID and the original receipt without replay.
    'test_kimi_native.NativeKimi.test_expired_cache_compacts_before_delivering_one_original_message',
    'test_kimi_native.NativeKimi.test_manual_compaction_reaches_native_hooks',
    'test_kimi_native.NativeKimi.test_pause_and_resume_keep_the_conversation',
    'test_kimi_native.NativeKimi.test_fork_requires_a_closed_source_instance_and_preserves_saved_context',
    'test_kimi_native.NativeKimi.test_rolling_window_429_recovers_in_the_same_conversation',
    'test_kimi_questions_native.NativeKimiTrust.test_a_short_unviewed_pane_grows_until_the_whole_prompt_shows_then_trusts_once',
    'test_kimi_questions_native.NativeKimiTrust.test_declining_trust_exits_kimi',
    'test_kimi_questions_native.NativeKimiTrust.test_a_viewed_short_pane_is_never_resized_or_answered',
    'test_kimi_questions_native.NativeKimiQuestions.test_tall_question_in_an_unviewed_80x24_pane_grows_until_answerable',
    'test_kimi_questions_native.NativeKimiQuestions.test_tall_question_with_an_attached_client_is_never_resized',
    'test_kimi_questions_native.NativeKimiQuestions.test_short_question_answers_without_growth',
    'test_kimi_questions_native.NativeKimiQuestions.test_short_question_long_other_is_confirmed_from_the_complete_review',
    'test_kimi_questions_native.NativeKimiQuestions.test_short_question_explicit_retry_resumes_the_existing_other_editor',
    'test_kimi_questions_native.NativeKimiQuestions.test_short_question_foreign_other_is_preserved_without_submission',
    'test_kimi_native.NativeKimi.test_id_settings_before_first_message_preserves_empty_conversation_and_defaults',
    'test_kimi_native.NativeKimi.test_id_unprimed_pending_settings_then_message_makes_one_model_call_and_keeps_defaults',
    'test_kimi_native.NativeKimi.test_id_effort_preserves_raw_request_identity_and_shared_defaults',
)
# The Agent Teams class (package 15) runs as its own child with its own bound, selected
# with --teams: one team of Kimi sessions launched by a private desktop service. Its
# account choice uses a recorded inspection stand-in (Kimi signs in only through
# api.kimi.com); everything after the choice is the real Kimi image.
TEAMS_METHODS = (
    'test_teams_native.NativeTeamsKimi.test_1_launch_and_identity',
    'test_teams_native.NativeTeamsKimi.test_2_delivery_and_member_shell',
    'test_teams_native.NativeTeamsKimi.test_3_lead_doorbell',
    'test_teams_native.NativeTeamsKimi.test_4_cancel_notice',
    'test_teams_native.NativeTeamsKimi.test_5_stop_and_terminate',
)
TEAMS_CASE_SECONDS = 2400
LOG_LIMIT = 64 * 1024**2
IMAGE_LIMIT = 256 * 1024**2
CASE_SECONDS = 900


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def distinct(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key')
        result[key] = value
    return result


def strict(data):
    return json.loads(data, object_pairs_hook=distinct)


def write(path, value):
    with Path(path).open('x', encoding='utf-8') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())


def target(system=None, machine=None):
    pair = (system or platform.system(), (machine or platform.machine()).lower())
    found = {('Linux', 'x86_64'): 'linux-x64', ('Darwin', 'arm64'): 'darwin-arm64'}.get(pair)
    if found is None:
        raise ValueError('only the reviewed Linux x64 and Darwin arm64 images are qualified')
    return found


discovery_spec = importlib.util.spec_from_file_location(
    'zerus_native_discovery', Path(__file__).with_name('native_discovery.py'))
discovery = importlib.util.module_from_spec(discovery_spec)
discovery_spec.loader.exec_module(discovery)

OfficialRedirect = discovery.OfficialRedirect


def metadata(url, limit, method='GET', *, policy='kimi'):
    return discovery.fetch_metadata(url, limit, method=method, policy=policy, stage='official Kimi metadata')


PACKAGE = '@moonshot-ai/kimi-code'
NPM_LATEST = 'https://registry.npmjs.org/' + PACKAGE + '/latest'
GITHUB_LATEST = 'https://api.github.com/repos/MoonshotAI/kimi-code/releases/latest'
STABLE_VERSION = r'(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})'


def stable_version(value):
    if not isinstance(value, str) or re.fullmatch(STABLE_VERSION, value) is None:
        raise ValueError('official current release is not one bounded stable version')
    return value


def current_release():
    raw, standalone_fetch = metadata('https://code.kimi.com/kimi-code/latest', 64)
    standalone = stable_version(raw.decode('ascii').strip())
    raw, npm_fetch = metadata(NPM_LATEST, 65536, policy='no-redirect')
    npm = strict(raw)
    if not isinstance(npm, dict) or npm.get('name') != PACKAGE:
        raise ValueError('official npm current release names another package')
    npm_version = stable_version(npm.get('version'))
    raw, github_fetch = metadata(GITHUB_LATEST, 262144, policy='no-redirect')
    github = strict(raw)
    if (not isinstance(github, dict) or github.get('draft') is not False
            or github.get('prerelease') is not False):
        raise ValueError('official GitHub current release is not published stable metadata')
    tag = github.get('tag_name')
    if not isinstance(tag, str) or not tag.startswith(PACKAGE + '@'):
        raise ValueError('official GitHub current release names another package')
    github_version = stable_version(tag[len(PACKAGE) + 1:])
    location = github.get('html_url')
    if not isinstance(location, str):
        raise ValueError('official GitHub current release has no text page URL')
    page = urllib.parse.urlsplit(location)
    if (page.scheme != 'https' or page.netloc != 'github.com' or page.query or page.fragment
            or urllib.parse.unquote(page.path) != '/MoonshotAI/kimi-code/releases/tag/' + tag):
        raise ValueError('official GitHub current release page differs from its tag')
    versions = dict(standalone=standalone, npm=npm_version, github=github_version)
    version = max(versions.values(), key=lambda value: tuple(map(int, value.split('.'))))
    agreeing = [name for name, observed in versions.items() if observed == version]
    if len(agreeing) < 2:
        raise ValueError('official current-release disagreement: the highest version is not corroborated')
    # Never drop a failed source or fall back to an older agreeing pair. Each
    # source is read once; every later artifact URL uses this one exact decision.
    sources = {name: dict(version=versions[name], fetch=fetch) for name, fetch in
               [('standalone', standalone_fetch), ('npm', npm_fetch), ('github', github_fetch)]}
    return version, dict(version=version, agreeing_sources=agreeing, sources=sources)


def resolve_latest(platform_id):
    version, selection = current_release()
    prefix = 'https://code.kimi.com/kimi-code/binaries/' + version + '/'
    raw, manifest_fetch = metadata(prefix + 'manifest.json', 65536)
    manifest = strict(raw)
    filename = 'kimi-code-' + platform_id + ('.exe' if platform_id.startswith('win32-') else '')
    entry = manifest['platforms'][platform_id]
    if (manifest['version'] != version or manifest['tag'] != '@moonshot-ai/kimi-code@' + version
            or entry['filename'] != filename or not isinstance(entry['checksum'], str)
            or re.fullmatch('[0-9a-f]{64}', entry['checksum']) is None):
        raise ValueError('latest manifest target/version/hash differs')
    _, image_head = metadata(prefix + filename, IMAGE_LIMIT, method='HEAD')
    record = dict(schema=1, version=version, target=platform_id,
                  manifest_sha256=hashlib.sha256(raw).hexdigest(), image_sha256=entry['checksum'],
                  image_size=image_head['size'])
    return record, raw, dict(latest=selection['sources']['standalone']['fetch'], selection=selection,
                           manifest=manifest_fetch, image_head=image_head)


def stage_latest(images, evidence, platform_id):
    record, manifest, discovery = resolve_latest(platform_id)
    path = images / 'manifest.json'
    with path.open('xb') as stream:
        stream.write(manifest)
        stream.flush()
        os.fsync(stream.fileno())
    path.chmod(0o400)
    filename = 'kimi-code-' + platform_id + ('.exe' if platform_id.startswith('win32-') else '')
    image = images / filename
    url = 'https://code.kimi.com/kimi-code/binaries/' + record['version'] + '/' + filename
    discovery['image'] = fetch(url, image, record['image_size'], record['image_sha256'])
    record_path = images / 'selected-release.json'
    write(record_path, record)
    record_path.chmod(0o400)
    write(evidence / 'selected-release-fetch.json', discovery)
    return record, image, record_path


def fetch(url, output, size, sha256, opener=None, redirects=None):
    """One GET, no retry, exclusive non-executable file, exact bounded bytes/hash."""
    if not 0 < size <= IMAGE_LIMIT:
        raise ValueError('download size is not bounded')
    redirects = redirects or OfficialRedirect()
    opener = opener or urllib.request.build_opener(urllib.request.ProxyHandler({}), redirects)
    end, received, checksum = time.monotonic() + 180, 0, hashlib.sha256()
    request = urllib.request.Request(url, headers={'Accept-Encoding': 'identity'}, method='GET')
    with opener.open(request, timeout=30) as response:
        lengths = response.headers.get_all('Content-Length', [])
        if (response.status != 200 or lengths != [str(size)]
                or response.headers.get('Content-Encoding', 'identity') != 'identity'):
            raise ValueError('download status/length/encoding differs from its pin')
        fd = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, 'O_NOFOLLOW', 0), 0o600)
        with os.fdopen(fd, 'wb') as stream:
            while True:
                if time.monotonic() >= end:
                    raise TimeoutError('download deadline')
                chunk = response.read(min(65536, size - received + 1))
                if time.monotonic() >= end:
                    raise TimeoutError('late download chunk')
                if not chunk:
                    break
                received += len(chunk)
                if received > size:
                    raise ValueError('download is oversized')
                stream.write(chunk)
                checksum.update(chunk)
            stream.flush()
            os.fsync(stream.fileno())
        if received != size or checksum.hexdigest() != sha256:
            raise ValueError('download size/hash differs from its pin')
        Path(output).chmod(0o400)
        return dict(url=url, final_url=response.url, redirects=redirects.hops,
                    size=received, sha256=checksum.hexdigest())


def checked_pins(platform_id):
    pins = strict(Path(__file__).with_name('kimi-native-pins.json').read_bytes())
    if set(pins) != set(HISTORICAL_VERSIONS):
        raise ValueError('the fixed reviewed versions changed')
    for version in HISTORICAL_VERSIONS:
        manifest = Path(__file__).with_name('kimi-native-manifests') / (version + '.json')
        observed = manifest.read_bytes()
        observed_sha256 = hashlib.sha256(observed).hexdigest()
        if observed_sha256 != pins[version]['manifest_sha256']:
            raise ValueError('retained official manifest changed: ' + json.dumps(dict(
                version=version, expected_sha256=pins[version]['manifest_sha256'],
                observed_sha256=observed_sha256, size=len(observed),
                crlf=observed.count(b'\r\n'), lf=observed.count(b'\n'), cr=observed.count(b'\r')),
                sort_keys=True))
        document = strict(observed)
        entry = document['platforms'][platform_id]
        pin = pins[version][platform_id]
        if (document['version'] != version or document['tag'] != '@moonshot-ai/kimi-code@' + version
                or entry['filename'] != 'kimi-code-' + platform_id + ('.exe' if platform_id.startswith('win32-') else '')
                or entry['checksum'] != pin['sha256']
                or type(pin['size']) is not int or not 0 < pin['size'] <= IMAGE_LIMIT):
            raise ValueError('official manifest and reviewed image pin disagree')
    return pins


def checked_teams_methods():
    """What will run: NativeTeamsKimi's test methods all come from TeamsContract, its
    base, and equal the reviewed list; either class missing is an explicit refusal."""
    tree = ast.parse((ROOT / 'tests' / 'test_teams_native.py').read_text())
    classes = {item.name: item for item in tree.body if isinstance(item, ast.ClassDef)}
    for name in ('TeamsContract', 'NativeTeamsKimi'):
        if name not in classes:
            raise ValueError('the Teams class ' + name + ' is missing')
    kimi = classes['NativeTeamsKimi']
    if 'TeamsContract' not in [base.id for base in kimi.bases if isinstance(base, ast.Name)]:
        raise ValueError('NativeTeamsKimi must take its cases from TeamsContract')
    if any(isinstance(item, ast.FunctionDef) and item.name.startswith('test_') for item in kimi.body):
        raise ValueError('Teams method inventory needs an explicit review')
    found = ['test_teams_native.NativeTeamsKimi.' + item.name for item in classes['TeamsContract'].body
             if isinstance(item, ast.FunctionDef) and item.name.startswith('test_')]
    if found != list(TEAMS_METHODS):
        raise ValueError('Teams method inventory needs an explicit review')
    return TEAMS_METHODS


def checked_methods():
    found = []
    for filename, classes in (('test_kimi_native', ('NativeKimi',)),
                              ('test_kimi_questions_native', ('NativeKimiTrust', 'NativeKimiQuestions'))):
        tree = ast.parse((ROOT / 'tests' / (filename + '.py')).read_text())
        for name in classes:
            node = next(item for item in tree.body if isinstance(item, ast.ClassDef) and item.name == name)
            found.extend(filename + '.' + name + '.' + item.name for item in node.body
                         if isinstance(item, ast.FunctionDef) and item.name.startswith('test_'))
    if len(found) != len(METHODS) or set(found) != set(METHODS):
        raise ValueError('native method inventory needs an explicit review')
    return METHODS


def source_map():
    names = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split('\0')
    return {name: digest(ROOT / name) for name in names if name}


def hosted_paths(args, env):
    """Accidental owner-machine invocation refuses before allocation or chmod."""
    if env.get('GITHUB_ACTIONS') != 'true' or env.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise ValueError('this campaign requires a disposable GitHub-hosted runner')
    temporary = Path(env['RUNNER_TEMP']).resolve()
    workspace = Path(env['GITHUB_WORKSPACE']).resolve()
    if workspace != ROOT:
        raise ValueError('the checkout is not the declared workspace')
    if any(not path.resolve().is_relative_to(temporary) for path in (args.hgs, args.tmux)):
        raise ValueError('hgs and tmux must be private runner-temporary builds')
    # The Teams class takes no Effort seed (--teams refuses one); the original cases require it.
    if not getattr(args, 'teams', False) and (getattr(args,'effort_seed_receipt',None) is None
                                              or not args.effort_seed_receipt.resolve().is_relative_to(temporary)):
        raise ValueError('private Effort seed receipt must stay in runner temporary builds')
    if not args.output.resolve().is_relative_to(workspace / 'artifacts' / 'test-results'):
        raise ValueError('results must stay in the checkout artifact directory')


def environment(runtime, helpers, hgs):
    # No parent credentials, provider variables, proxies or native run bindings.
    env = dict(PATH=str(helpers) + ':/usr/bin:/bin', LANG='C.UTF-8', PYTHONUTF8='1',
               PYTHONDONTWRITEBYTECODE='1', PYTHONUNBUFFERED='1', HGS_TEST_BIN=str(hgs))
    for key, name in (('HOME', 'home'), ('TMPDIR', 'tmp'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_DATA_HOME', 'data'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_STATE_HOME', 'state'), ('XDG_RUNTIME_DIR', 'runtime')):
        path = runtime / name
        path.mkdir(mode=0o700)
        env[key] = str(path)
    return env


def seed_helper():
    spec=importlib.util.spec_from_file_location('native_effort_seed',Path(__file__).with_name('native-effort-seed.py'))
    module=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def seed_input(args):
    value=seed_helper().verify(ROOT,args.effort_seed_receipt,args.hgs)
    if not Path(value['image']['path']).resolve(strict=True).is_relative_to(Path(os.environ['RUNNER_TEMP']).resolve()):
        raise ValueError('private Effort seed image must stay in runner temporary builds')
    return value


def record_roots(case, destination):
    """Only declared fixture attributes, never a /tmp scan or cleanup authority."""
    for name in ('root', 'images', 'image', 'kimi'):
        value = getattr(case, name, None)
        if not isinstance(value, (str, Path)):
            continue
        path = Path(value).resolve()
        if not path.exists():
            continue
        if path.is_file():
            path = path.parent
        if not path.name.startswith(('hgs-pause-', 'hgs-kimi-image-', 'hgs-kimi-native-', 'hgs-tn-')):
            raise ValueError('fixture root is not a declared private temporary root')
        info = path.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
            raise ValueError('fixture root is not private')
        record = dict(case=case.id(), path=str(path), device=info.st_dev, inode=info.st_ino, uid=info.st_uid)
        with Path(destination).open('a', encoding='utf-8') as stream:
            stream.write(json.dumps(record) + '\n')
            stream.flush()
            os.fsync(stream.fileno())


def teams_custody(custody_path):
    """The Teams fixture's early custody: its private root is recorded the moment it
    exists, so a child killed during setup or in a step still names it."""
    class Declared:
        def __init__(self, case, root):
            self.case, self.root = case, root

        def id(self):
            return 'test_teams_native.' + self.case

    return lambda case, root: record_roots(Declared(case, root), custody_path)


def suite_child(result_path, custody_path, teams=False):
    import unittest
    sys.path.insert(0, str(ROOT / 'tests'))
    if teams:
        import teams_native_fixture
        teams_native_fixture.ROOT_CREATED = teams_custody(custody_path)
        return run_child(result_path, custody_path, TEAMS_METHODS, unittest)
    import test_kimi_native as guard
    import test_kimi_questions_native as questions

    def observe(original):
        def setting(case, name, value):
            original(case, name, value)
            if name in ('root', 'images', 'image', 'kimi'):
                record_roots(case, custody_path)
        return setting
    # Metadata-only delegation: fixture setup, predicates and assertions stay
    # unchanged. Record early assignments too, so a setup timeout keeps roots.
    for cls in (guard.NativeKimi, questions.NativeKimi):
        cls.__setattr__ = observe(cls.__setattr__)
    return run_child(result_path, custody_path, METHODS, unittest)


def run_child(result_path, custody_path, methods, unittest):
    started, completed = [], []

    class Result(unittest.TextTestResult):
        def startTest(self, case):
            started.append(case.id())
            super().startTest(case)

        def stopTest(self, case):
            record_roots(case, custody_path)
            completed.append(case.id())
            super().stopTest(case)

    suite = unittest.defaultTestLoader.loadTestsFromNames(methods)
    result = unittest.TextTestRunner(verbosity=2, failfast=True, resultclass=Result).run(suite)
    value = dict(tests=result.testsRun, started=started, completed=completed,
                 failures=len(result.failures), errors=len(result.errors), skipped=len(result.skipped),
                 expected_failures=len(result.expectedFailures), unexpected_successes=len(result.unexpectedSuccesses),
                 failed_cases=[case.id() for case, _ in result.failures],
                 error_cases=[case.id() for case, _ in result.errors],
                 skipped_cases=[case.id() for case, _ in result.skipped])
    write(result_path, value)
    return 0 if outcome(value, 0, methods) else 1


def outcome(result, code, methods=METHODS):
    return (code == 0 and type(result.get('tests')) is int and result['tests'] == len(methods) and result.get('started') == list(methods)
            and result.get('completed') == list(methods)
            and all(type(result.get(key)) is int and result[key] == 0 for key in
                    ('failures', 'errors', 'skipped', 'expected_failures', 'unexpected_successes')))


def failure_summary(result, methods=METHODS):
    details = {key: result.get(key) for key in ('tests', 'failures', 'errors', 'skipped')}
    for key in ('failed_cases', 'error_cases', 'skipped_cases'):
        details[key] = result.get(key, [])
    return ('native result did not qualify all %d exact cases without skips/errors: %s'
            % (len(methods), json.dumps(details, sort_keys=True)))


def await_child(child, log, seconds=CASE_SECONDS):
    end = time.monotonic() + seconds
    while child.poll() is None:
        if time.monotonic() >= end or Path(log).stat().st_size > LOG_LIMIT:
            # Only this direct unreaped child, no PID/tree/group fallback.
            child.terminate()
            try:
                child.wait(10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(10)
            raise TimeoutError('suite deadline/log cap; detached fixture cleanup is UNKNOWN')
        time.sleep(.1)
    if Path(log).stat().st_size > LOG_LIMIT:
        raise ValueError('suite log cap')
    return child.returncode


def retain(custody, runtime, archive):
    """Bounded diagnostic snapshot only; unknown writers are not a stable seal."""
    roots = {str(runtime): None}
    for path in custody:
        if not path.exists():
            continue
        if path.stat().st_size > 256 * 1024:
            raise ValueError('custody metadata cap')
        for line in path.read_text().splitlines():
            row = strict(line)
            root = Path(row['path'])
            if root.exists():
                info = root.lstat()
                if (info.st_dev, info.st_ino, info.st_uid) != (row['device'], row['inode'], row['uid']):
                    raise ValueError('retained root identity changed')
                roots[str(root)] = row
    count, size = 0, 0
    with tarfile.open(archive, 'x:gz', dereference=False) as output:
        def add(parent, name, member):
            nonlocal count, size
            info = os.stat(name, dir_fd=parent, follow_symlinks=False)
            count += 1
            size += info.st_size if stat.S_ISREG(info.st_mode) else 0
            if count > 20000 or size > 2 * 1024**3 or info.st_uid != os.getuid():
                raise ValueError('retained runtime cap/owner')
            item = tarfile.TarInfo(member)
            item.mode, item.mtime = stat.S_IMODE(info.st_mode), int(info.st_mtime)
            if stat.S_ISLNK(info.st_mode):
                item.type, item.linkname = tarfile.SYMTYPE, os.readlink(name, dir_fd=parent)
                if len(item.linkname) > 4096:
                    raise ValueError('retained link cap')
                output.addfile(item)
                return
            if not (stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode)):
                raise ValueError('special file in retained runtime')
            flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
            if stat.S_ISDIR(info.st_mode):
                flags |= os.O_DIRECTORY
            fd = os.open(name, flags, dir_fd=parent)
            try:
                opened = os.fstat(fd)
                if (opened.st_dev, opened.st_ino, opened.st_mode, opened.st_size) != (info.st_dev, info.st_ino, info.st_mode, info.st_size):
                    raise ValueError('retained entry changed before open')
                if stat.S_ISDIR(info.st_mode):
                    item.type = tarfile.DIRTYPE
                    output.addfile(item)
                    for child in os.listdir(fd):
                        add(fd, child, member + '/' + child)
                else:
                    item.size = info.st_size
                    with os.fdopen(os.dup(fd), 'rb') as stream:
                        output.addfile(item, stream)
            finally:
                os.close(fd)
        for index, root in enumerate(map(Path, roots)):
            parent = os.open(root.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try:
                add(parent, root.name, 'root-' + str(index))
            finally:
                os.close(parent)
    return dict(roots=roots, members=count, regular_bytes=size, consistency='diagnostic snapshot; unknown writers may exist')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--hgs', required=True, type=Path)
    parser.add_argument('--tmux', required=True, type=Path)
    parser.add_argument('--effort-seed-receipt', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--teams', action='store_true', help='Run only the Agent Teams Kimi class, in its own child')
    args = parser.parse_args()
    hosted_paths(args, os.environ)
    if args.teams and args.effort_seed_receipt is not None:
        raise ValueError('the Teams class takes no Effort seed')
    effort_seed = None if args.teams else seed_input(args)
    os.umask(0o077)
    if not re.fullmatch('[0-9a-f]{40}', args.source_sha):
        raise ValueError('an exact source SHA is required')
    if subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip() != args.source_sha:
        raise ValueError('source changed')
    # Validate the reviewed inventory, then use the fixed list itself.
    if args.teams:
        checked_teams_methods()
    else:
        checked_methods()
    methods = TEAMS_METHODS if args.teams else METHODS
    platform_id = target()
    if args.teams and not platform_id.startswith('linux-'):
        # Before any release is staged or the reservation is spent.
        raise ValueError('the Teams preview exists only on Linux in this milestone; --teams needs a Linux runner')
    out = args.output.resolve()
    out.parent.mkdir(parents=True, exist_ok=True)
    out.mkdir(mode=0o700)
    evidence = out / 'evidence'
    evidence.mkdir(mode=0o700)
    images = out / 'images'
    images.mkdir(mode=0o700)
    runtime = Path(tempfile.mkdtemp(prefix='kn.', dir='/tmp')).resolve()
    helpers = runtime / 'helpers'
    helpers.mkdir(mode=0o700)
    (helpers / 'python3').symlink_to(Path(sys.executable).resolve())
    (helpers / 'tmux').symlink_to(args.tmux.resolve(strict=True))
    hgs = args.hgs.resolve(strict=True)
    hgs.chmod(0o500)
    env = environment(runtime, helpers, hgs)
    if effort_seed is not None:
        env.update(seed_helper().environment(effort_seed))
    source, hgs_sha = source_map(), digest(hgs)
    tools = {str(path): digest(path) for path in (Path(sys.executable).resolve(), args.tmux.resolve(), Path('/usr/bin/cc'))}
    write(evidence / 'plan.json', dict(source_sha=args.source_sha, platform=platform_id, methods=methods,
          native_cases=len(methods), mac_shim_compiles=len(methods) if platform_id == 'darwin-arm64' and not args.teams else 0,
          hgs_sha256=hgs_sha, tools=tools, environment=env, runtime=str(runtime), no_retries=True))
    write(evidence / 'effort-seed.json',effort_seed)
    write(evidence / 'source-before.json', source)
    records, custody, success = [], [], False
    try:
        release, image, record_path = stage_latest(images, evidence, platform_id)
        version = release['version']
        selected_seals = {str(path): digest(path) for path in (image, record_path, images / 'manifest.json')}
        write(evidence / 'reservation.json', dict(state='SPENT', versions=[version], native_cases=len(methods),
              selected_release=release, time=time.time()))
        if source_map() != source or digest(hgs) != hgs_sha or (effort_seed is not None and seed_input(args)!=effort_seed):
            raise ValueError('source or hgs changed before native execution')
        case_evidence = evidence / ('native-' + version)
        case_evidence.mkdir(mode=0o700)
        result, owned = evidence / (version + '-result.json'), evidence / (version + '-custody.jsonl')
        custody.append(owned)
        native_env = dict(env, HGS_KIMI_TEST_BIN=str(image), HGS_KIMI_TEST_SHA256=release['image_sha256'], HGS_KIMI_TEST_RECORD=str(record_path),
                          HGS_KIMI_GUARD=str(image),
                          ZERUS_KIMI_GUARD=str(image),
                          HGS_KIMI_EVIDENCE_DIR=str(case_evidence))
        if args.teams:
            native_env.update(ZERUS_TEAMS_KIMI_HOSTED='1', ZERUS_TEAMS_EVIDENCE=str(case_evidence / 'teams'))
        argv = [sys.executable, '-s', '-B', str(Path(__file__).resolve()), '--teams-child' if args.teams else '--suite-child',
                str(result), str(owned)]
        write(evidence / (version + '-command.json'), dict(argv=argv, environment=native_env))
        log = evidence / (version + '.log')
        with log.open('xb') as stream:
            child = subprocess.Popen(argv, env=native_env, cwd=ROOT, stdin=subprocess.DEVNULL,
                                     stdout=stream, stderr=stream, start_new_session=True)
            code = await_child(child, log, TEAMS_CASE_SECONDS if args.teams else CASE_SECONDS)
        value = strict(result.read_bytes())
        if not outcome(value, code, methods):
            raise ValueError(failure_summary(value, methods))
        records.append(value)
        if (source_map() != source or digest(hgs) != hgs_sha or (effort_seed is not None and seed_input(args)!=effort_seed)
                or any(digest(path) != sha for path, sha in tools.items())):
            raise ValueError('source/binary/tool post-seal changed')
        if any(digest(path) != sha for path, sha in selected_seals.items()):
            raise ValueError('a staged image changed')
        success = True
    except BaseException as error:
        write(evidence / 'failure.json', dict(type=type(error).__name__, message=str(error), cleanup='UNKNOWN; ephemeral VM is the outer boundary'))
        try:
            write(evidence / 'retention.json', retain(custody, runtime, out / 'unknown-runtime.tar.gz'))
        except BaseException as retention_error:
            write(evidence / 'retention-error.json', dict(type=type(retention_error).__name__, message=str(retention_error)))
        raise
    finally:
        write(evidence / 'receipt.json', dict(source=args.source_sha, platform=platform_id, outcome='PASS' if success else 'FAIL',
              completed_versions=len(records), native_cases=len(methods) if success else None, images_retained=str(images), runtime_retained=str(runtime),
              owner_install=False, historical_np50_authority=False))
    return 0


if __name__ == '__main__':
    if len(sys.argv) == 4 and sys.argv[1] in ('--suite-child', '--teams-child'):
        raise SystemExit(suite_child(Path(sys.argv[2]), Path(sys.argv[3]), teams=sys.argv[1] == '--teams-child'))
    raise SystemExit(main())
