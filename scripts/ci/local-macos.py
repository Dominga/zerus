#!/usr/bin/env python3
"""Run exact-commit Mac checks in private homes under the shared build lock.

GitHub artifact transport is replaced by a local, hash-bound producer. A local
result never admits a published artifact, installed pair, or release candidate.
"""
import sys

if __name__ == '__main__' and not sys.flags.isolated:
    # Before loading anything from the caller's path, select the same public
    # interpreter without cwd, PYTHONPATH or user-site package participation.
    import os
    os.execv(sys.executable, [sys.executable, '-I', __file__, *sys.argv[1:]])

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import shlex
import site
import subprocess
import sysconfig
import types
import time
import tarfile
import tempfile
import urllib.request

CI = None  # Initialized only from verified source bytes during preparation.
SUPPORTED = {'client-native-macos-core', 'client-native-macos-tauri', 'connector-macos',
             'cli-macos-debug', 'cli-macos-core', 'cli-macos-native', 'clippy-macos'}
RUST_CACHE = ('echo "rustc=$(rustc{toolchain} -Vv | git hash-object --stdin | cut -c1-16)" >> "$GITHUB_OUTPUT"\n'
              'echo "epoch=$(date -u +%Y-%m)" >> "$GITHUB_OUTPUT"')
TOOLCHAINS = {'rustup toolchain install 1.99.1 --profile minimal',
              'rustup toolchain install 1.99.1 --profile minimal --component clippy',
              'rustup toolchain install stable --profile minimal && rustup default stable'}
TRANSPORT = {
    'python3 scripts/ci/macos-debug-artifact.py seal --directory artifacts/mac-cli-debug',
    'rustup toolchain install "$MAC_DEBUG_RUST" --profile minimal && rustup default "$MAC_DEBUG_RUST"',
    'python3 scripts/ci/macos-debug-artifact.py admit --artifact-id "$MAC_DEBUG_ID" \\\n  --manifest-sha256 "$MAC_DEBUG_SHA" --producer-attempt "$MAC_DEBUG_ATTEMPT" --rust-release "$MAC_DEBUG_RUST"',
    'python3 scripts/ci/macos-debug-artifact.py install --directory "$RUNNER_TEMP/mac-cli-debug" \\\n  --artifact-id "$MAC_DEBUG_ID" --manifest-sha256 "$MAC_DEBUG_SHA" \\\n  --producer-attempt "$MAC_DEBUG_ATTEMPT" --rust-release "$MAC_DEBUG_RUST" --check-rust',
    'python3 scripts/ci/macos-debug-artifact.py install --directory "$RUNNER_TEMP/mac-cli-debug" \\\n  --artifact-id "$MAC_DEBUG_ID" --manifest-sha256 "$MAC_DEBUG_SHA" \\\n  --producer-attempt "$MAC_DEBUG_ATTEMPT" --rust-release "$MAC_DEBUG_RUST"',
}


def plan(workflow, cli, selected=None):
    result = []
    for key, job in workflow['jobs'].items():
        row = dict(id=key, name=job.get('name', key), status='not run', steps=[])
        result.append(row)
        if key == 'ios':
            row['reason'] = 'iOS global Xcode selection and device services are not isolated locally; GitHub only'
            continue
        if key not in SUPPORTED or (selected is not None and key not in selected):
            row['reason'] = 'other platform or GitHub aggregate' if selected is None else 'not selected'
            continue
        if job.get('runs-on') != 'macos-15':
            row['reason'] = 'unsupported workflow runner'
            continue
        defaults = {**workflow.get('defaults', {}).get('run', {}), **job.get('defaults', {}).get('run', {})}
        for index, step in enumerate(job['steps']):
            name = step.get('name', step.get('run', step.get('uses', 'step'))).splitlines()[0]
            item = dict(name=name, index=index, status='not run')
            row['steps'].append(item)
            body = step.get('run', '')
            action = step.get('uses', '')
            if action and body:
                item['reason'] = 'mixed action and command; no infrastructure substitution'
                continue
            if 'select-xcode.sh' in body or re.search(r'\bxcode-select\b', body):
                item['reason'] = 'global Xcode selection excluded on owner hardware; GitHub only'
                continue
            if action.startswith(('actions/checkout@', 'actions/cache/', 'actions/upload-artifact@',
                                  'actions/download-artifact@')) or body.rstrip('\n') in TRANSPORT:
                item.update(status='local', reason='local exact-source producer/cache/logs replace GitHub transport')
                continue
            if action.startswith('actions/setup-node@'):
                major = str(step.get('with', {}).get('node-version', ''))
                item.update(status='local' if major == '24' else 'not run',
                            reason='private Node24 supplied' if major == '24' else 'unsupported Node major ' + major)
                continue
            if action:
                item['reason'] = 'unsupported action'
                continue
            if body.rstrip('\n') == 'brew install tmux python' or body.startswith('rustup toolchain install'):
                if key == 'ios' and '--target' in body:
                    body = body.replace('rustup toolchain install 1.99.1 --profile minimal --target',
                                        'rustup target add --toolchain 1.99.1').replace(',', ' ')
                elif body.rstrip('\n') == 'brew install tmux python' or body.rstrip('\n') in TOOLCHAINS:
                    item.update(status='local', reason='preflight supplies tools; host package/toolchain mutation excluded')
                    continue
            if body.rstrip('\n') in {RUST_CACHE.format(toolchain=t) for t in ('', ' +1.99.1')}:
                item.update(status='local', reason='exclusive local caches; no GitHub cache identity')
                continue
            condition = step.get('if', '')
            if condition not in ('', 'always()', 'failure()'):
                item['reason'] = 'unsupported condition: ' + condition
                continue
            if step.get('shell', defaults.get('shell', 'bash')) != 'bash' or not body or '${{' in body:
                item['reason'] = 'unsupported shell/body/expression'
                continue
            units = [(name, body)]
            if key == 'cli-macos-debug' and body.strip() == 'bash scripts/ci/cli.sh debug':
                builds = []
                for title, command in [('Rebuild exact-SHA CLI', 'cargo build --locked'),
                                       ('Build exact-SHA fake agent', 'cargo build --locked --example fake_agent')]:
                    if cli.splitlines().count(command) != 1:
                        raise RuntimeError('unrecognized exact Mac CLI build recipe')
                    builds.append((title, command + '\n'))
                units = [*builds, (name, body)]
                row['steps'].pop()
            if body.strip() == 'bash scripts/ci/cli.sh remaining':
                units = CI.cli_units(cli)[1:]
                row['steps'].pop()
            for part, (title, script) in enumerate(units):
                current = item if len(units) == 1 else dict(item, name=title, part=part)
                env = {**workflow.get('env', {}), **job.get('env', {}), **step.get('env', {})}
                # The artifact identity is replaced only for this local producer.
                for field in ('MAC_DEBUG_ID', 'MAC_DEBUG_SHA', 'MAC_DEBUG_ATTEMPT', 'MAC_DEBUG_RUST'):
                    env.pop(field, None)
                current.update(body=script, body_sha256=CI.digest(script.encode()), env=env,
                               directory=step.get('working-directory', defaults.get('working-directory', '.')),
                               timeout=min(3600, int(step.get('timeout-minutes', job.get('timeout-minutes', 30))) * 60),
                               condition=condition)
                if len(units) != 1:
                    row['steps'].append(current)
    return result


def quoted(path):
    return json.dumps(str(path))


def mktemp_script():
    # Explicit paths/options retain BSD mktemp's own meaning. Only an implicit
    # parent is redirected into the already-private job TMPDIR.
    return ('#!/bin/sh\n'
            'for zerus_arg do\n'
            '  case "$zerus_arg" in */*|-p|-p?*) exec /usr/bin/mktemp "$@";; esac\n'
            'done\n'
            '[ -n "$TMPDIR" ] && [ -d "$TMPDIR" ] || exit 125\n'
            'exec /usr/bin/mktemp -p "$TMPDIR" "$@"\n')


def login_shell_script(observation_bin):
    # /etc/profile runs path_helper before the command, moving /bin/ps ahead
    # of our non-setuid copy. Preserve real login startup and every original
    # argument; restore only the private observation tool before that command.
    prefix = 'PATH=' + shlex.quote(str(observation_bin)) + ':"$PATH"; export PATH\n'
    return ('#!/bin/sh\n'
            'if [ "$#" -ge 2 ] && [ "$1" = "-lic" ]; then\n'
            '  zerus_original=$2\n'
            '  shift 2\n'
            '  zerus_prefix=' + shlex.quote(prefix) + '\n'
            '  exec /bin/bash -lic "$zerus_prefix$zerus_original" "$@"\n'
            'fi\n'
            'exec /bin/bash "$@"\n')


def verify_runner_source(repository, commit, directory):
    paths = subprocess.check_output(['git', '-C', str(repository), 'ls-tree', '-r', '--name-only',
                                     commit, 'scripts/ci'], text=True).splitlines()
    expected = {Path(p).name: p for p in paths if Path(p).parent == Path('scripts/ci') and Path(p).suffix == '.py'}
    present = {p.name: p for p in directory.glob('*.py')}
    required = {name for name in expected if name.startswith('local')}
    if not required <= present.keys() or not present.keys() <= expected.keys():
        raise ValueError('runner directory has missing or unrecorded Python helpers')
    verified = {}
    for name, path in present.items():
        recorded = subprocess.check_output(['git', '-C', str(repository), 'show', commit + ':' + expected[name]])
        if path.is_symlink() or path.read_bytes() != recorded:
            raise ValueError('runner helper differs from its immutable commit: ' + name)
        verified[name] = recorded
    return verified


def load_ci_sources(verified, directory):
    """Compile the verified bytes; never ask Python to load caller bytecode."""
    # Discard the caller's cwd, PYTHONPATH and helper/package shadows. Public
    # interpreter/tool packages remain prerequisites, independently of the tree.
    paths = sysconfig.get_paths()
    trusted = {str(Path(paths[k]).resolve()) for k in ('stdlib', 'platstdlib', 'purelib', 'platlib')}
    trusted.update(str(Path(p).resolve()) for p in site.getsitepackages())
    if sysconfig.get_config_var('DESTSHARED'):
        trusted.add(str(Path(sysconfig.get_config_var('DESTSHARED')).resolve()))
    for prefix in (sys.prefix, sys.base_prefix):
        prefix = Path(prefix).resolve()
        archive = 'python' + str(sys.version_info.major) + str(sys.version_info.minor) + '.zip'
        trusted.update((str(prefix / 'lib' / archive), str(prefix / archive), str(prefix / 'DLLs')))
    sys.path = sorted(trusted)
    import yaml
    if not any(Path(yaml.__file__).resolve().is_relative_to(Path(p)) for p in trusted):
        raise ValueError('PyYAML came from a caller package shadow')
    # Preload every local import from the retained, already-verified byte map.
    # compile() neither reads nor writes __pycache__, including valid stale pyc.
    for filename in ('local_network.py', 'local_services.py', 'local_container.py', 'local-linux.py'):
        name = filename.removesuffix('.py').replace('-', '_')
        module = types.ModuleType(name)
        module.__file__ = str(directory / filename)
        sys.modules[name] = module
        try:
            exec(compile(verified[filename], module.__file__, 'exec', dont_inherit=True), module.__dict__)
        except BaseException:
            sys.modules.pop(name, None)
            raise
    return sys.modules['local_linux']


def node24(root):
    target = root / 'node24'
    if target.exists():
        if subprocess.check_output([str(target / 'bin/node'), '--version'], text=True).startswith('v24.'):
            return target / 'bin'
        raise RuntimeError('private Node cache has the wrong major')
    base = 'https://nodejs.org/dist/latest-v24.x/'
    with urllib.request.urlopen(base + 'SHASUMS256.txt', timeout=30) as response:
        rows = [line.split() for line in response.read().decode().splitlines()
                if re.fullmatch(r'[0-9a-f]{64}  node-v24\.[0-9]+\.[0-9]+-darwin-arm64.tar.gz', line)]
    if len(rows) != 1:
        raise RuntimeError('official Mac Node archive did not resolve uniquely')
    expected, filename = rows[0]
    archive = root / 'node24.tar.gz'
    with urllib.request.urlopen(base + filename, timeout=120) as response, archive.open('wb') as stream:
        shutil.copyfileobj(response, stream)
    if CI.file_digest(archive) != expected:
        raise RuntimeError('official Mac Node checksum mismatch')
    staging = root / 'node24-unpack'; staging.mkdir()
    with tarfile.open(archive) as bundle:
        bundle.extractall(staging, filter='data')
    (staging / filename.removesuffix('.tar.gz')).rename(target)
    staging.rmdir(); archive.unlink()
    (root / 'node24.json').write_text(json.dumps(dict(url=base + filename, sha256=expected)))
    return target / 'bin'


class Canary:
    """An exclusive private outside-sandbox object, retained through cleanup."""
    def __init__(self, parent):
        self.directory = Path(tempfile.mkdtemp(prefix='zerus-ci-canary-', dir=parent))
        self.directory_stamp = self.directory.stat(follow_symlinks=False)
        self.directory_fd = os.open(self.directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        self.path = self.directory / 'canary'
        self.fd, self.stamp = None, None
        try:
            held = os.fstat(self.directory_fd)
            if (held.st_dev, held.st_ino) != (self.directory_stamp.st_dev, self.directory_stamp.st_ino):
                raise RuntimeError('canary directory changed before exclusive creation')
            self.fd = os.open('canary', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                              0o600, dir_fd=self.directory_fd)
            self.stamp = os.fstat(self.fd)
            text = b'synthetic out-of-sandbox canary'
            if os.write(self.fd, text) != len(text):
                raise OSError('short canary write')
        except BaseException:
            self.close()
            raise

    def close(self):
        """Never unlink a replacement leaf or a substituted directory."""
        unchanged = False
        try:
            selected = self.directory.stat(follow_symlinks=False)
            if (selected.st_dev, selected.st_ino) != (self.directory_stamp.st_dev, self.directory_stamp.st_ino):
                return False
            if self.stamp is not None:
                leaf = os.stat('canary', dir_fd=self.directory_fd, follow_symlinks=False)
                if (leaf.st_dev, leaf.st_ino) != (self.stamp.st_dev, self.stamp.st_ino):
                    return False
                os.unlink('canary', dir_fd=self.directory_fd)
            os.rmdir(self.directory)
            unchanged = True
            return True
        except OSError:
            return unchanged
        finally:
            if self.fd is not None:
                os.close(self.fd); self.fd = None
            os.close(self.directory_fd)


# These are policy checks only, not bootstrap lookups or keychain item requests.
# SecurityServer and securityd/secd MachServices come from Apple's public system
# launch plists; deny the whole related namespace rather than one API spelling.
CREDENTIAL_SERVICES = ('com.apple.SecurityServer', 'com.apple.securityd.xpc',
                       'com.apple.securityd.general', 'com.apple.secd',
                       'com.apple.security.octagon', 'com.apple.security.kcsharing')
CREDENTIAL_PROBE = '''
import ctypes,os
library=ctypes.CDLL('/usr/lib/libsandbox.1.dylib')
check=library.sandbox_check
check.argtypes=(ctypes.c_int,ctypes.c_char_p,ctypes.c_int)
check.restype=ctypes.c_int
for service in CREDENTIAL_SERVICES:
    # SANDBOX_FILTER_GLOBAL_NAME=2, as declared by WebKit's SandboxSPI.h.
    assert check(os.getpid(),b'mach-lookup',2,ctypes.c_char_p(service.encode())) > 0, service
assert check(os.getpid(),b'mach-lookup',2,ctypes.c_char_p(b'com.apple.cfprefsd.agent')) == 0
print('keychain-service policy denies; no IPC or keychain item query')
'''.replace('CREDENTIAL_SERVICES', repr(CREDENTIAL_SERVICES))


def profile(root, public, temporary, system_temp=None):
    # Seatbelt does not implement counted repetitions such as {32}. Spell out
    # the exact length; neither a shorter nor a longer fixture name is admitted.
    fixture_hex = '[0-9a-f]' * 32
    service_hex = '[0-9a-f]' * 16
    reads = ['/System', '/usr', '/bin', '/sbin', '/opt/homebrew', '/Library/Developer',
             '/Library/Apple', '/Applications/Xcode.app', '/private/etc', '/private/var/db/dyld',
             '/Library/Preferences/com.apple.dt.Xcode.plist',
             '/Library/Preferences/com.apple.dt.CommandLineTools.plist',
             '/private/var/db/xcode_select_link']
    parents = list(Path(root).parents)
    rules = [
        '(version 1)', '(deny default)',
        '(allow process-exec process-fork sysctl-read mach-lookup mach-task-name)',
        '(deny mach-lookup (global-name "com.apple.SecurityServer") '
        '(global-name-regex #"^com\\.apple\\.(securityd|secd)(\\.|$)") '
        '(global-name-regex #"^com\\.apple\\.security\\."))',
        '(allow signal (target same-sandbox))',
        '(allow process-info*)',
        '(allow file-read-metadata)',
        '(allow file-read* (literal "/private") (literal "/private/tmp"))',
        '(allow file-read* ' + ' '.join('(subpath ' + quoted(p) + ')' for p in reads + public) + ')',
        '(allow file-read* ' + ' '.join('(literal ' + quoted(p) + ')' for p in parents) + ')',
        '(allow file-read* file-write* (subpath ' + quoted(root) + ') (subpath ' + quoted(temporary) + ') (subpath "/dev"))',
        '(allow file-ioctl (subpath "/dev"))',
        '(allow file-read* file-write* (regex #"^/private/tmp/(zguard|zgui)-' + fixture_hex + '(/|$)"))',
        '(allow file-read* file-write* (regex #"^/private/tmp/hgs-svc-' + service_hex + '(/|$)"))',
        '(allow network-outbound (remote ip))',
        '(allow network-inbound network-bind (local ip))',
        '(allow network* (subpath ' + quoted(temporary) + ') (subpath ' + quoted(root) + '))',
        '(allow network* (regex #"^/private/tmp/(zguard|zgui)-' + fixture_hex + '(/|$)"))',
        '(allow network* (regex #"^/private/tmp/hgs-svc-' + service_hex + '(/|$)"))',
        '(allow network-outbound (literal "/private/var/run/mDNSResponder"))',
    ]
    if system_temp is not None:
        # Apple's compiler/Python shims use confstr's per-user OS cache even
        # with TMPDIR set. Permit only public xcrun cache names, never that
        # directory recursively or the user's preferences/credentials.
        path = Path(system_temp)
        if not str(path).startswith('/private/var/folders/') or path.name != 'T':
            raise RuntimeError('unrecognized OS Xcode cache parent')
        rules.append('(allow file-read* ' + ' '.join('(literal ' + quoted(p) + ')' for p in [path, *path.parents]) + ')')
        pattern = '^' + re.escape(str(path / 'xcrun_db')) + '(?:-|$)'
        rules.append('(allow file-read* file-write* (regex #' + quoted(pattern) + '))')
    return '\n'.join(rules) + '\n'


class Runner:
    def __init__(self, args, report):
        self.args, self.report = args, report
        self.output = args.output.resolve()
        self.root = args.root.resolve()
        self.cache = self.root / 'cache'
        self.target = self.root / 'target'
        self.snapshot = self.output / 'snapshot'
        self.temporary = Path('/private/tmp') / ('zmc-' + str(os.getpid()))
        self.owner = 'migrate-u zerus-hn7fg ' + str(os.getpid())
        self.lock = Path.home() / 'zerus-dev/.heavy.lock'
        self.held = False
        self.producer = None
        self.producer_builds = {}
        self.environment = {}

    def prepare(self):
        self.lock.mkdir()
        (self.lock / 'owner').write_text(self.owner + '\n')
        self.held = True
        self.output.mkdir(parents=True, mode=0o700)
        harness = self.output / 'harness'; harness.mkdir()
        for path in Path(__file__).parent.glob('local*.py'):
            shutil.copy2(path, harness / path.name)
        self.report['harness_files'] = {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in harness.iterdir()}
        self.root.chmod(0o700)
        self.cache.mkdir(exist_ok=True)
        self.target.mkdir(exist_ok=True)
        self.host_environment = {'PATH': '/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin',
                                 'HOME': str(self.root), 'GIT_CONFIG_NOSYSTEM': '1', 'GIT_CONFIG_GLOBAL': '/dev/null'}
        subprocess.run(['git', '-c', 'core.hooksPath=/dev/null', 'clone', '--shared', '--no-checkout',
                        str(self.args.repo), str(self.snapshot)], env=self.host_environment, check=True, timeout=180)
        subprocess.run(['git', '-C', str(self.snapshot), '-c', 'credential.helper=', 'fetch', '--no-tags',
                        'https://github.com/ufna/zerus.git', self.report['commit']], env=self.host_environment, check=True, timeout=300)
        subprocess.run(['git', '-C', str(self.snapshot), '-c', 'core.hooksPath=/dev/null', 'checkout',
                        '--detach', self.report['commit']], env=self.host_environment, check=True)
        runner_commit = self.args.runner_sha or self.report['commit']
        if runner_commit != self.report['commit']:
            subprocess.run(['git', '-C', str(self.snapshot), '-c', 'credential.helper=', 'fetch', '--no-tags',
                            'https://github.com/ufna/zerus.git', runner_commit],
                           env=self.host_environment, check=True, timeout=300)
        verified = verify_runner_source(self.snapshot, runner_commit, Path(__file__).parent)
        global CI
        CI = load_ci_sources(verified, Path(__file__).parent)
        self.report['runner_source_files'] = {name: hashlib.sha256(data).hexdigest() for name, data in verified.items()}
        self.report['runner_commit'] = runner_commit
        self.node = node24(self.cache)
        self.temporary.mkdir(mode=0o700)
        self.tools = self.root / 'tools'
        self.tools.mkdir(exist_ok=True)
        # BSD mktemp otherwise uses confstr's real per-user directory even
        # with TMPDIR supplied. Preserve its native implementation and options,
        # while making the default parent this job's verified private folder.
        mktemp = self.tools / 'mktemp'
        mktemp.write_text(mktemp_script())
        mktemp.chmod(0o755)
        for name in ('cargo', 'rustc', 'rustdoc', 'rustfmt', 'cargo-fmt', 'cargo-clippy', 'clippy-driver', 'rustup'):
            source = Path.home() / '.cargo/bin' / name
            dest = self.tools / name
            if not dest.exists():
                shutil.copy2(source.resolve(), dest)
        toolchain = Path('/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin')
        for name, selected in {'cc': 'clang', 'c++': 'clang++', 'clang': 'clang', 'clang++': 'clang++',
                               'ar': 'ar', 'ranlib': 'ranlib', 'swift': 'swift', 'swiftc': 'swiftc'}.items():
            destination = self.tools / name
            if not destination.exists():
                destination.symlink_to(toolchain / selected)
        xcode = self.tools / 'xcodebuild'
        if not xcode.exists():
            xcode.symlink_to('/Applications/Xcode.app/Contents/Developer/usr/bin/xcodebuild')
        # macOS ps is setuid and cannot execute inside Seatbelt. Keep the exact
        # system implementation in our private tools, without setuid or private
        # Apple entitlements. Ad-hoc signing uses no identity/keychain and lets
        # this copy read ordinary current-user kernel information in the sandbox.
        ps = self.tools / 'ps'
        shutil.copyfile('/bin/ps', ps)
        ps.chmod(0o755)
        entitlements = self.tools / 'empty-entitlements.plist'
        entitlements.write_text('<?xml version="1.0"?><plist version="1.0"><dict/></plist>')
        subprocess.run(['/usr/bin/codesign', '--force', '--sign', '-', '--timestamp=none',
                        '--entitlements', str(entitlements), str(ps)], check=True, timeout=30)
        self.report['private_ps'] = dict(system_sha256=CI.file_digest(Path('/bin/ps')),
                                         sandbox_copy_sha256=CI.file_digest(ps),
                                         mode=oct(ps.stat().st_mode & 0o7777),
                                         note='system code; setuid removed; empty entitlements; ad-hoc signature')
        observations = self.tools / 'observations'; observations.mkdir(exist_ok=True)
        shutil.copy2(ps, observations / 'ps')
        shells = self.tools / 'login-shells'; shells.mkdir(exist_ok=True)
        self.login_shell = shells / 'bash'
        self.login_shell.write_text(login_shell_script(observations))
        self.login_shell.chmod(0o755)
        self.report['private_login_shell'] = dict(sha256=CI.file_digest(self.login_shell),
            note='real bash login startup; original command/arguments; private ps routing only')
        rustup = self.cache / 'rustup'
        if not rustup.exists():
            rustup.mkdir()
            (rustup / 'toolchains').mkdir()
            subprocess.run(['cp', '-cR', str(Path.home() / '.rustup/toolchains/1.99.1-aarch64-apple-darwin'),
                            str(rustup / 'toolchains')], check=True)
        (rustup / 'settings.toml').write_text('version = "12"\ndefault_toolchain = "1.99.1-aarch64-apple-darwin"\nprofile = "minimal"\nauto_self_update = "disable"\n')
        cargo = self.cache / 'cargo'
        cargo.mkdir(exist_ok=True)
        if not (cargo / 'registry').exists():
            subprocess.run(['cp', '-cR', str(Path.home() / '.cargo/registry'), str(cargo)], check=True)
        for manifest in self.snapshot.rglob('Cargo.toml'):
            path = manifest.parent / 'target'
            if not path.exists():
                path.symlink_to(self.target)
        objects = (self.snapshot / '.git/objects/info/alternates').read_text().splitlines()
        self.sandbox = self.output / 'checks.sb'
        system_temp = Path(subprocess.check_output(['/usr/bin/getconf', 'DARWIN_USER_TEMP_DIR'], text=True).strip()).resolve()
        self.sandbox.write_text(profile(self.root, objects, self.temporary, system_temp))
        self.report['sandbox_sha256'] = CI.file_digest(self.sandbox)
        self.report['tools'] = {}
        for name, command in {'rust': [str(self.tools / 'rustc'), '+1.99.1', '-Vv'],
                              'tmux': ['/opt/homebrew/bin/tmux', '-V'],
                              'node': [str(self.node / 'node'), '--version']}.items():
            p = subprocess.run(command, capture_output=True, text=True, timeout=30,
                               env={**self.host_environment, 'RUSTUP_HOME': str(rustup)})
            self.report['tools'][name] = dict(exit_code=p.returncode, version=p.stdout.strip())
            if p.returncode:
                raise RuntimeError('required tool failed: ' + name)
        # Node must match the workflow, not the owner's current Node.
        self.node_major = self.report['tools']['node']['version'].lstrip('v').split('.')[0]

    def check(self, job, step, workspace, home):
        env = {k: str(v).replace('${{ needs.source.outputs.commit }}', self.report['commit']) for k, v in step['env'].items()}
        if any('${{' in v or k.startswith('HGS_') or k == 'ZERUS_CLIENT_DESKTOP_SERVICE' for k, v in env.items()):
            step.update(status='not run', reason='unsupported or authority-bearing environment')
            return
        if 'cli-macos' in job['id'] and job['id'] != 'cli-macos-debug':
            if not self.producer or any(CI.file_digest(self.target / leaf) != self.producer[key] for leaf, key in
                                       [('debug/hgs', 'hgs_sha256'), ('debug/examples/fake_agent', 'fake_agent_sha256')]):
                step.update(status='not run', reason='same-source local debug producer is unavailable or changed')
                return
        deadline = time.monotonic() + 600
        while os.getloadavg()[0] > self.args.max_load:
            if time.monotonic() >= deadline:
                step.update(status='not run', reason='load ceiling held for ten minutes')
                return
            time.sleep(10)
        if shutil.disk_usage(self.root).free < self.args.min_free_gib * 1024**3:
            step.update(status='not run', reason='disk floor; no new work started')
            return
        name = f"{job['id']}-{step['index']:02d}-{step.get('part', 0)}"
        script = self.output / (name + '.sh')
        script.write_text('set -euo pipefail\nmkdir -p artifacts/test-results\n' + step['body'])
        log = self.output / (name + '.log')
        private_tmp = self.temporary / job['id']
        private_tmp.mkdir(mode=0o700, exist_ok=True)
        run_temp = home / 'runner-temp'
        run_temp.mkdir(exist_ok=True)
        base_env = dict(HOME=str(home), USER='test', LOGNAME='test', LANG='en_US.UTF-8', SHELL=str(self.login_shell),
                   CFFIXED_USER_HOME=str(home),
                   PATH=str(self.tools) + ':' + str(self.node) + ':/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin',
                   TMPDIR=str(private_tmp), TMUX_TMPDIR=str(private_tmp),
                   ZERUS_TEST_SHORT_TMPDIR=str(private_tmp),
                   XDG_CONFIG_HOME=str(home / '.config'), XDG_STATE_HOME=str(home / '.local/state'),
                   XDG_DATA_HOME=str(home / '.local/share'), XDG_CACHE_HOME=str(home / '.cache'),
                   CARGO_HOME=str(self.cache / 'cargo'), RUSTUP_HOME=str(self.cache / 'rustup'),
                   RUSTUP_TOOLCHAIN='1.99.1', CARGO_TARGET_DIR=str(self.target),
                   CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0',
                   CARGO_BUILD_JOBS=str(self.args.workers), RUST_TEST_THREADS=str(self.args.workers),
                   DEVELOPER_DIR='/Applications/Xcode.app/Contents/Developer',
                   SDKROOT='/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk',
                   CLANG_MODULE_CACHE_PATH=str(private_tmp / 'clang-modules'),
                   SWIFT_MODULECACHE_PATH=str(private_tmp / 'swift-modules'),
                   ZERUS_PYTHON_JOBS=str(self.args.workers), ZERUS_TEST_JOBS=str(self.args.workers),
                   HOMEBREW_NO_AUTO_UPDATE='1', GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null',
                   GITHUB_WORKSPACE=str(workspace), RUNNER_TEMP=str(run_temp),
                   GITHUB_OUTPUT=str(home / 'github-output'), GITHUB_ENV=str(home / 'github-env'))
        base_env.update(env)
        env = base_env
        directory = Path(step['directory'])
        if directory.is_absolute() or '..' in directory.parts:
            step.update(status='not run', reason='working directory escapes checkout')
            return
        step.update(status='running', log=log.name, started_at=time.time())
        CI.save(self.output / 'report.json', self.report)
        print('RUN', job['id'], step['name'], flush=True)
        with log.open('wb') as stream:
            p = subprocess.Popen(['/usr/bin/nice', '-n', '10', '/usr/bin/sandbox-exec', '-f', str(self.sandbox),
                                  '/bin/bash', '-e', '-o', 'pipefail', str(script)], cwd=workspace / directory,
                                 env=env, stdin=subprocess.DEVNULL, stdout=stream, stderr=subprocess.STDOUT,
                                 start_new_session=True)
            deadline = time.monotonic() + step['timeout']
            try:
                while True:
                    try:
                        code = p.wait(timeout=min(2, max(0.01, deadline - time.monotonic())))
                        break
                    except subprocess.TimeoutExpired:
                        if time.monotonic() > deadline or shutil.disk_usage(self.root).free < self.args.min_free_gib * 1024**3:
                            raise RuntimeError('step deadline or disk floor; original fixture group stopped')
            except BaseException:
                if p.poll() is None:
                    os.killpg(p.pid, signal.SIGKILL)
                p.wait(timeout=15)
                raise
        step.update(status='pass' if code == 0 else 'fail', exit_code=code,
                    seconds=round(time.time() - step['started_at'], 2), log_sha256=CI.file_digest(log))
        print(step['status'].upper(), job['id'], step['name'], step['seconds'], flush=True)

    def finish(self):
        if self.held and (self.lock / 'owner').read_text().strip() == self.owner:
            (self.lock / 'owner').unlink()
            self.lock.rmdir()
            self.held = False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True, help='public local source object cache')
    parser.add_argument('--sha', required=True)
    parser.add_argument('--runner-sha', help='immutable reviewed runner commit when it differs from --sha')
    parser.add_argument('--root', type=Path, required=True, help='one private cache/source/output root')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--workers', type=int, choices=range(1, 7), default=6)
    parser.add_argument('--max-load', type=float, default=10)
    parser.add_argument('--min-free-gib', type=int, default=10)
    parser.add_argument('--jobs')
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('requires macOS')
    if not re.fullmatch(r'[0-9a-f]{40}', args.sha):
        parser.error('use a complete immutable commit hash')
    if args.runner_sha is not None and not re.fullmatch(r'[0-9a-f]{40}', args.runner_sha):
        parser.error('use a complete immutable runner commit hash')
    args.root.mkdir(parents=True, exist_ok=True, mode=0o700)
    if args.output.exists() or not args.output.resolve().is_relative_to(args.root.resolve()):
        parser.error('output must be new and below the private root')
    # Bodies are transferred from the exact source separately during preparation.
    report = dict(version=1, commit=args.sha, source_commit=args.sha, runner_commit=None,
                  isolated_interpreter=bool(sys.flags.isolated),
                  platform='native macOS local preflight', completed=False, jobs=[])
    runner = Runner(args, report)
    try:
        runner.prepare()
        workflow_bytes = (runner.snapshot / CI.WORKFLOW).read_bytes()
        workflow = CI.load_workflow(workflow_bytes)
        selected = set(args.jobs.split(',')) if args.jobs else None
        report.update(workflow_sha256=CI.digest(workflow_bytes), runner_sha256=CI.file_digest(Path(__file__)),
                      jobs=plan(workflow, (runner.snapshot / 'scripts/ci/cli.sh').read_text(), selected))
        # Real unrelated fixtures, never the owner's files or processes. Refuse
        # all checks unless reads/signals outside this sandbox are denied.
        canary = Canary('/private/tmp')
        report['canary_created_exclusively'] = True
        unrelated = subprocess.Popen(['/bin/sleep', '60'])
        probe_body = '''python3 - <<'PYPROBE'
import os,pathlib,signal,subprocess
assert not any(k.startswith('HGS_') for k in os.environ)
try:pathlib.Path(CANARY).read_text()
except PermissionError:pass
else:raise AssertionError('outside file contents readable')
try:os.kill(UNRELATED,signal.SIGUSR1)
except PermissionError:pass
else:raise AssertionError('outside process signal allowed')
p=subprocess.Popen(['/bin/sleep','20']);p.terminate();p.wait(timeout=5)
print('private home; outside content/signal refused; own child reaped')
PYPROBE
'''.replace('CANARY', repr(str(canary.path))).replace('UNRELATED', str(unrelated.pid))
        probe_body += "python3 - <<'PYCREDENTIAL'\n" + CREDENTIAL_PROBE + "\nPYCREDENTIAL\n"
        probe = dict(name='Mac sandbox startup', index=-1, env={}, directory='.', timeout=30,
                     body=probe_body)
        probe_job = dict(id='sandbox', steps=[probe])
        probe_home = args.output / 'probe-home'; probe_home.mkdir()
        try:
            runner.check(probe_job, probe, runner.snapshot, probe_home)
        finally:
            canary_clean = canary.close()
            report['canary_original_cleanup'] = canary_clean
            if unrelated.poll() is None:
                unrelated.terminate()
            unrelated.wait(timeout=5)
            if not canary_clean:
                raise RuntimeError('canary changed; replacement retained, no tests started')
        report['isolation_probe'] = probe
        if probe['status'] != 'pass':
            raise RuntimeError('Mac sandbox startup failed; no tests started')
        order = ['cli-macos-debug', 'cli-macos-core', 'cli-macos-native',
                 'client-native-macos-core', 'client-native-macos-tauri', 'connector-macos', 'ios']
        for job in sorted(report['jobs'], key=lambda row: order.index(row['id']) if row['id'] in order else len(order)):
            if not job['steps']:
                continue
            workspace = args.output / 'checkouts' / job['id']
            workspace.parent.mkdir(exist_ok=True)
            subprocess.run(['cp', '-cR', str(runner.snapshot), str(workspace)], check=True)
            home = args.output / 'homes' / job['id']; home.mkdir(parents=True, mode=0o700)
            for step in job['steps']:
                if 'body' not in step:
                    continue
                try:
                    runner.check(job, step, workspace, home)
                except (OSError, RuntimeError) as error:
                    step.update(status='fail', reason=str(error))
                if job['id'] == 'cli-macos-debug' and step['status'] == 'pass':
                    names = {'Rebuild exact-SHA CLI': ('hgs_sha256', 'debug/hgs'),
                             'Build exact-SHA fake agent': ('fake_agent_sha256', 'debug/examples/fake_agent')}
                    if step['name'] in names:
                        key, leaf = names[step['name']]
                        runner.producer_builds[key] = CI.file_digest(runner.target / leaf)
                    if len(runner.producer_builds) == 2:
                        runner.producer = dict(commit=args.sha, **runner.producer_builds)
                        report['local_debug_producer'] = runner.producer
                job['status'] = CI.summarize(job['steps'])
                CI.save(args.output / 'report.json', report)
        report['completed'] = True
    except BaseException as error:
        report['preparation_error'] = str(error)
        raise
    finally:
        if args.output.exists():
            if CI is None:
                (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
            else:
                CI.save(args.output / 'report.json', report)
        runner.finish()
    print('Report:', args.output / 'report.json')
    return 1 if any(j['status'] == 'fail' for j in report['jobs']) else 2


if __name__ == '__main__':
    sys.exit(main())
