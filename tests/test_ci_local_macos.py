"""Mac planning must preserve checks and the local producer's exact boundary."""
import importlib.util
from pathlib import Path
import sys
import os
import json
import re
import subprocess
import ast
import py_compile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('local_mac_ci', ROOT / 'scripts/ci/local-macos.py')
MAC = importlib.util.module_from_spec(SPEC)
YAML_MISSING = False
with patch.object(sys, 'path', [str(ROOT / 'scripts/ci'), *sys.path]):
    SPEC.loader.exec_module(MAC)
    try:
        MAC.CI = MAC.load_ci_sources({p.name: p.read_bytes() for p in (ROOT / 'scripts/ci').glob('local*.py')}, ROOT / 'scripts/ci')
    except ModuleNotFoundError as error:
        if error.name != 'yaml':
            raise
        YAML_MISSING = True


def setUpModule():
    # The local Mac runner refuses to plan without PyYAML; hosted Mac Python has none.
    if YAML_MISSING:
        raise unittest.SkipTest('the local Mac planner requires PyYAML')


class MacPlan(unittest.TestCase):
    def workflow(self, steps):
        return {'jobs': {'cli-macos-debug': {'runs-on': 'macos-15', 'steps': steps}}}

    def test_compound_or_renamed_infrastructure_keeps_its_real_body(self):
        bodies = ['echo cache; exit 19', 'brew install tmux python; exit 19',
                  'rustup toolchain install stable --profile minimal && rustup default stable; exit 19',
                  next(iter(MAC.TRANSPORT)) + '\necho MISSED; exit 19']
        rows = MAC.plan(self.workflow([dict(name='Rust cache identity', run=b) for b in bodies]), '')[0]['steps']
        self.assertEqual([row.get('body') for row in rows], bodies)
        for body in bodies:
            row = MAC.plan(self.workflow([dict(name='Seal the tested debug executables and their provenance', run=body)]), '')[0]['steps'][0]
            self.assertEqual(row.get('body'), body)

    def test_transport_substitution_requires_the_whole_known_body(self):
        for body in MAC.TRANSPORT:
            with self.subTest(body=body):
                row = MAC.plan(self.workflow([dict(run=body)]), '')[0]['steps'][0]
                self.assertEqual(row['status'], 'local')
                self.assertNotIn('body', row)
        row = MAC.plan(self.workflow([dict(uses='actions/checkout@123', run='exit 19')]), '')[0]['steps'][0]
        self.assertEqual(row['status'], 'not run')

    def test_independent_remaining_suites_keep_their_original_source(self):
        steps = [dict(run='bash scripts/ci/cli.sh remaining')]
        workflow = {'jobs': {'cli-macos-core': {'runs-on': 'macos-15', 'steps': steps}}}
        rows = MAC.plan(workflow, (ROOT / 'scripts/ci/cli.sh').read_text())[0]['steps']
        self.assertEqual([row['name'] for row in rows], ['CLI shell smoke', 'CLI Python integration', 'CLI release build'])
        self.assertIn('python3 scripts/ci/python-tests.py', rows[1]['body'])
        self.assertIn('voice_recovery_fixture', rows[1]['body'])

    def test_exact_binary_builds_do_not_depend_on_a_passing_unit_suite(self):
        rows = MAC.plan(self.workflow([dict(run='bash scripts/ci/cli.sh debug')]),
                        (ROOT / 'scripts/ci/cli.sh').read_text())[0]['steps']
        self.assertEqual([row['body'] for row in rows], ['cargo build --locked\n',
                         'cargo build --locked --example fake_agent\n', 'bash scripts/ci/cli.sh debug'])

    def test_unknown_node_major_leaves_a_gap(self):
        step = {'uses': 'actions/setup-node@123', 'with': {'node-version': '26'}}
        row = MAC.plan(self.workflow([step]), '')[0]['steps'][0]
        self.assertEqual(row['status'], 'not run')

    def test_ios_and_selection_calls_stay_not_run_in_every_context(self):
        body = 'bash mobile/ios/scripts/select-xcode.sh'
        contexts = ({}, {'working-directory': 'alternate'}, {'shell': 'pwsh'},
                    {'if': 'new_condition'}, {'env': {'BASH_ENV': 'alternate/startup.sh'}})
        for context in contexts:
            with self.subTest(context=context):
                job = {'runs-on': 'macos-15', 'steps': [dict(run=body, **context)]}
                row = MAC.plan({'jobs': {'ios': job}}, '')[0]
                self.assertEqual(row['status'], 'not run')
                self.assertEqual(row['steps'], [])
                self.assertIn('GitHub only', row['reason'])
                # A selection helper moved into another supported job is also
                # refused, including mixed bodies; it is never a local pass.
                job['steps'][0]['run'] += '\nexit 19'
                rows = MAC.plan(self.workflow(job['steps']), '')[0]['steps']
                self.assertEqual(rows[0]['status'], 'not run')
                self.assertNotIn('body', rows[0])
        job = {'runs-on': 'macos-15', 'defaults': {'run': {'working-directory': 'alternate'}},
               'steps': [{'run': body}]}
        self.assertEqual(MAC.plan({'jobs': {'ios': job}}, '')[0]['steps'], [])

    def test_signal_and_file_scope_cannot_expand_to_the_owner_home(self):
        profile = MAC.profile('/Users/example/zerus-dev/private-ci', ['/public-git/objects'], '/private/tmp/owned-ci')
        self.assertIn('(allow signal (target same-sandbox))', profile)
        self.assertNotIn('(allow signal)', profile)
        self.assertNotIn('(subpath "/Users/example")', profile)
        self.assertIn('(literal "/private/tmp")', profile)
        self.assertNotIn('(subpath "/private/tmp")', profile)
        self.assertNotIn('com.hgdev.hgs-tray', profile)
        self.assertIn('(allow file-ioctl (subpath "/dev"))', profile)
        self.assertIn('(deny mach-lookup (global-name "com.apple.SecurityServer")', profile)
        self.assertIn('securityd|secd', profile)

    def test_xcode_cache_exception_is_only_the_public_cache_leaf(self):
        profile = MAC.profile('/Users/example/private-ci', [], '/private/tmp/owned-ci',
                              '/private/var/folders/ab/synthetic-user/T')
        self.assertIn('xcrun_db', profile)
        self.assertNotIn('(subpath "/private/var/folders', profile)
        with self.assertRaises(RuntimeError):
            MAC.profile('/Users/example/private-ci', [], '/private/tmp/owned-ci', '/Users/example')

    def test_fixture_patterns_keep_exact_lengths_without_counted_repetitions(self):
        profile = MAC.profile('/Users/example/private-ci', [], '/private/tmp/owned-ci')
        patterns = [json.loads(p) for p in re.findall(r'\(regex #("[^"\n]+")\)', profile)]
        self.assertEqual(len(patterns), 4)
        for prefix, length in [('zguard-', 32), ('zgui-', 32), ('hgs-svc-', 16)]:
            for size in (length - 1, length, length + 1):
                path = '/private/tmp/' + prefix + 'a' * size
                for suffix in ('', '/socket'):
                    self.assertEqual(any(re.match(p, path + suffix) for p in patterns), size == length)
        self.assertTrue(all('{' not in p for p in patterns))

    @unittest.skipUnless(sys.platform == 'darwin', 'native Seatbelt policy control')
    def test_native_fixture_directory_and_socket_scope(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            policy = root / 'checks.sb'
            policy.write_text(MAC.profile(root, [str(Path(sys.base_prefix).resolve())], root))
            script = root / 'probe.py'
            script.write_text(MAC.CREDENTIAL_PROBE + '''
import pathlib, socket, uuid
for prefix, length in [('zguard-', 32), ('zgui-', 32), ('hgs-svc-', 16)]:
    for size in (length - 1, length, length + 1):
        token = uuid.uuid4().hex + uuid.uuid4().hex
        path = pathlib.Path('/private/tmp') / (prefix + token[:size])
        created = False
        try:
            try:
                path.mkdir(mode=0o700)
                created = True
            except PermissionError:
                assert size != length, (prefix, size, 'exact fixture refused')
            else:
                assert size == length, (prefix, size, 'adjacent length admitted')
                with socket.socket(socket.AF_UNIX) as stream:
                    stream.bind(str(path / 's'))
                (path / 's').unlink()
        finally:
            if created:
                path.rmdir()
print('three exact fixture directories/sockets; six adjacent lengths refused')
''')
            env = dict(HOME=str(root), CFFIXED_USER_HOME=str(root), TMPDIR=str(root),
                       PATH='/usr/bin:/bin', LANG='en_US.UTF-8')
            result = subprocess.run(['/usr/bin/sandbox-exec', '-f', str(policy), sys.executable, str(script)],
                                    env=env, cwd=root, capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn('six adjacent lengths refused', result.stdout)


class CanaryIdentity(unittest.TestCase):
    def test_exclusive_private_canary_and_original_cleanup(self):
        with tempfile.TemporaryDirectory() as parent:
            canary = MAC.Canary(parent)
            self.assertEqual(canary.directory.stat().st_mode & 0o777, 0o700)
            self.assertEqual(canary.path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(canary.path.read_text(), 'synthetic out-of-sandbox canary')
            self.assertTrue(canary.close())
            self.assertFalse(canary.directory.exists())

    def test_precreated_symlink_never_truncates_the_other_file(self):
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent)
            other = root / 'other'; other.write_text('keep exactly')
            precreated = root / 'precreated'; precreated.mkdir(mode=0o700)
            alias = precreated / 'canary'; alias.symlink_to(other)
            with patch.object(MAC.tempfile, 'mkdtemp', return_value=str(precreated)):
                with self.assertRaises(FileExistsError):
                    MAC.Canary(root)
            self.assertEqual(other.read_text(), 'keep exactly')
            self.assertTrue(alias.is_symlink())

    def test_replaced_leaf_is_retained_and_not_followed_during_cleanup(self):
        with tempfile.TemporaryDirectory() as parent:
            other = Path(parent) / 'other'; other.write_text('keep exactly')
            canary = MAC.Canary(parent)
            canary.path.unlink(); canary.path.symlink_to(other)
            self.assertFalse(canary.close())
            self.assertEqual(other.read_text(), 'keep exactly')
            self.assertTrue(canary.path.is_symlink())

    def test_replaced_parent_is_not_cleaned_up(self):
        with tempfile.TemporaryDirectory() as parent:
            canary = MAC.Canary(parent)
            original = Path(parent) / 'original'
            canary.directory.rename(original)
            canary.directory.mkdir(mode=0o700)
            other = canary.directory / 'other'; other.write_text('keep exactly')
            self.assertFalse(canary.close())
            self.assertEqual(other.read_text(), 'keep exactly')
            self.assertEqual((original / 'canary').read_text(), 'synthetic out-of-sandbox canary')


class PrivateTools(unittest.TestCase):
    def test_mktemp_preserves_every_explicit_template_and_parent(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            native = root / 'native'
            native.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
            native.chmod(0o755)
            wrapper = root / 'mktemp'
            wrapper.write_text(MAC.mktemp_script().replace('/usr/bin/mktemp', str(native)))
            wrapper.chmod(0o755)
            cases = [(['-d', '/private/tmp/name with space.XXXXXX'], False),
                     (['subdir/name.XXXXXX'], False), (['-p', str(root), 'name.XXXXXX'], False),
                     (['-p' + str(root), 'name.XXXXXX'], False),
                     (['-d', 'implicit.XXXXXX'], True), (['-t', 'prefix'], True)]
            for arguments, implicit in cases:
                with self.subTest(arguments=arguments):
                    result = subprocess.run([str(wrapper), *arguments], env={'TMPDIR': str(root)},
                                            capture_output=True, text=True, check=True)
                    expected = (['-p', str(root)] if implicit else []) + arguments
                    self.assertEqual(result.stdout.splitlines(), expected)

    def test_login_wrapper_keeps_command_and_positional_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            native = root / 'native'
            native.write_text('#!/usr/bin/env python3\nimport json,sys\nprint(json.dumps(sys.argv[1:]))\n')
            native.chmod(0o755)
            observation = root / "ps tools ' dollar$"
            wrapper = root / 'bash'
            wrapper.write_text(MAC.login_shell_script(observation).replace('/bin/bash', str(native)))
            wrapper.chmod(0o755)
            command = 'exec "$0" --run "$@"'
            original = [command, 'program with space', 'quote\'"$;\\', 'Русский']
            result = subprocess.check_output([str(wrapper), '-lic', *original], text=True)
            actual = json.loads(result)
            self.assertEqual(actual[0], '-lic')
            self.assertTrue(actual[1].endswith('\n' + command))
            self.assertEqual(actual[2:], original[1:])
            self.assertEqual(json.loads(subprocess.check_output([str(wrapper), '-c', command, 'zero'], text=True)),
                             ['-c', command, 'zero'])

    @unittest.skipUnless(sys.platform == 'darwin', 'native BSD mktemp and bash login startup')
    def test_native_templates_and_login_startup_keep_the_private_ps_route(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            home = root / 'home'; home.mkdir()
            (home / '.bash_profile').write_text('printf login > "$HOME/login-marker"\nPATH=/usr/bin:/bin\n')
            observations = root / 'observations'; observations.mkdir()
            ps = observations / 'ps'
            # Routing only; actual native ps under Seatbelt is a separate retained control.
            ps.write_text('#!/bin/sh\nprintf private-routing\n'); ps.chmod(0o755)
            shell = root / 'bash'; shell.write_text(MAC.login_shell_script(observations)); shell.chmod(0o755)
            env = {'HOME': str(home), 'TMPDIR': str(root), 'PATH': '/usr/bin:/bin'}
            command = 'ps; printf "\\n"; printf "%s\\n" "$0" "$@"'
            result = subprocess.run([str(shell), '-lic', command, 'original zero', 'arg $;\\'],
                                    env=env, capture_output=True, text=True, check=True)
            self.assertEqual(result.stdout.splitlines(), ['private-routing', 'original zero', 'arg $;\\'])
            self.assertEqual((home / 'login-marker').read_text(), 'login')
            wrapper = root / 'mktemp'; wrapper.write_text(MAC.mktemp_script()); wrapper.chmod(0o755)
            for arguments in (['-d', str(root / 'absolute.XXXXXX')], ['-d', 'implicit.XXXXXX'],
                              ['-d', '-p', str(root), 'explicit.XXXXXX']):
                created = Path(subprocess.check_output([str(wrapper), *arguments], env=env, text=True).strip())
                self.assertEqual(created.parent, root)
                created.rmdir()


class RunnerProvenance(unittest.TestCase):
    def test_real_loading_ignores_timestamp_valid_bytecode_and_package_shadows(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            helpers = root / 'scripts/ci'; helpers.mkdir(parents=True)
            for path in (ROOT / 'scripts/ci').glob('local*.py'):
                (helpers / path.name).write_bytes(path.read_bytes())
            git = ['git', '-C', str(root)]
            subprocess.run([*git, 'init', '-q'], check=True)
            subprocess.run([*git, 'add', '.'], check=True)
            subprocess.run([*git, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                            'commit', '-qm', 'reviewed helper bytes'], check=True)
            commit = subprocess.check_output([*git, 'rev-parse', 'HEAD'], text=True).strip()
            path = helpers / 'local-linux.py'; original = path.read_bytes()
            tree = ast.parse(original)
            node = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'summarize')
            lines = original.splitlines(keepends=True)
            span = b''.join(lines[node.lineno - 1:node.end_lineno])
            prefix = b'def summarize(steps):\n    return "pass"\n'
            replacement = prefix + b'#' + b'x' * (len(span) - len(prefix) - 2) + b'\n'
            self.assertEqual(len(replacement), len(span))
            path.write_bytes(b''.join(lines[:node.lineno - 1]) + replacement + b''.join(lines[node.end_lineno:]))
            os.utime(path, (1700000000, 1700000000))
            py_compile.compile(str(path), doraise=True, invalidation_mode=py_compile.PycInvalidationMode.TIMESTAMP)
            path.write_bytes(original); os.utime(path, (1700000000, 1700000000))
            shadow = helpers / 'yaml'; shadow.mkdir()
            (shadow / '__init__.py').write_text('raise AssertionError("caller package shadow ran")\n')
            # Test the actual module startup, verifier, loader and aggregator in
            # a fresh child, with the attack directory first on the caller path.
            driver = '''import importlib.util,json,subprocess,sys
from pathlib import Path
helpers=Path(sys.argv[1]);sys.path.insert(0,str(helpers))
spec=importlib.util.spec_from_file_location('actual_mac',helpers/'local-macos.py')
mac=importlib.util.module_from_spec(spec);spec.loader.exec_module(mac)
assert mac.CI is None, 'no helper may execute before verification'
sources=mac.verify_runner_source(helpers.parents[1],sys.argv[2],helpers)
# A later disk replacement must not replace the already verified byte map.
(helpers/'local_network.py').write_text('raise AssertionError("late helper replacement ran")\\n')
mac.CI=mac.load_ci_sources(sources,helpers)
assert mac.CI.load_workflow(b'jobs: {}') == {'jobs': {}}
code=subprocess.run(['/bin/bash','-c','exit 19']).returncode
step={'status':'fail' if code else 'pass','exit_code':code}
assert mac.CI.summarize([step])=='fail',step
print(json.dumps({'step':step,'job':'fail'}))
'''
            result = subprocess.run([sys.executable, '-c', driver, str(helpers), commit],
                                    capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout), {'step': {'status': 'fail', 'exit_code': 19}, 'job': 'fail'})
            for name in ('argparse', 'json'):
                (helpers / (name + '.py')).write_text('raise AssertionError("caller stdlib shadow ran")\n')
            result = subprocess.run([sys.executable, str(helpers / 'local-macos.py'), '--help'],
                                    env={**os.environ, 'PYTHONPATH': str(helpers)},
                                    capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('--runner-sha', result.stdout)

    def test_runner_commit_is_independent_of_source_checkout_and_rejects_shadows(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / 'repo'; repo.mkdir()
            def git(*arguments):
                return subprocess.check_output(['git', '-C', str(repo), *arguments], text=True).strip()
            git('init', '-q')
            helpers = repo / 'scripts/ci'; helpers.mkdir(parents=True)
            (helpers / 'local-macos.py').write_text('old runner\n')
            (repo / 'product').write_text('frozen\n')
            git('add', '.')
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'source')
            source = git('rev-parse', 'HEAD')
            (helpers / 'local-macos.py').write_text('fixed runner\n')
            git('add', '.')
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'runner')
            runner = git('rev-parse', 'HEAD')
            supplied = root / 'harness'; supplied.mkdir()
            (supplied / 'local-macos.py').write_text('fixed runner\n')
            git('checkout', '-q', '--detach', source)
            self.assertEqual(set(MAC.verify_runner_source(repo, runner, supplied)), {'local-macos.py'})
            self.assertEqual(git('rev-parse', 'HEAD'), source)
            self.assertEqual((repo / 'product').read_text(), 'frozen\n')
            (supplied / 'unittest.py').write_text('ignored shadow\n')
            with self.assertRaises(ValueError):
                MAC.verify_runner_source(repo, runner, supplied)
            (supplied / 'unittest.py').unlink()
            (supplied / 'local-macos.py').write_text('uncommitted replacement\n')
            with self.assertRaises(ValueError):
                MAC.verify_runner_source(repo, runner, supplied)
            (supplied / 'local-macos.py').unlink()
            with self.assertRaises(ValueError):
                MAC.verify_runner_source(repo, runner, supplied)


if __name__ == '__main__':
    unittest.main()
