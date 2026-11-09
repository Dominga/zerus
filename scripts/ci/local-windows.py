#!/usr/bin/env python3
"""Exact-source Windows preflight with private profiles and original job custody.

Owner-machine mode refuses installer/autostart, hosted adversaries, breakaway
and wildcard cleanup. Missing checks stay NOT RUN, never local-only passes.
"""
import argparse
import datetime
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

SPEC = importlib.util.spec_from_file_location('local_ci', Path(__file__).with_name('local-linux.py'))
CI = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(CI)
SUPPORTED = {'windows-tests', 'windows-desktop', 'windows-host', 'client-native-windows'}
TOOLCHAINS = {
    'rustup toolchain install stable --profile minimal && rustup default stable',
    'rustup toolchain install 1.99.1 --profile minimal --component clippy',
    'rustup toolchain install stable --profile minimal && rustup default stable && rustup toolchain install 1.99.1 --profile minimal --component clippy',
}


def plan(workflow, selected=None):
    jobs = []
    for key, source in workflow['jobs'].items():
        job = dict(id=key, name=source.get('name', key), status='not run', steps=[])
        jobs.append(job)
        if key not in SUPPORTED or (selected is not None and key not in selected):
            job['reason'] = 'other platform or not selected'
            continue
        if source.get('runs-on') != 'windows-2025':
            job['reason'] = 'unsupported workflow runner'
            continue
        defaults = source.get('defaults', {}).get('run', {})
        for index, step in enumerate(source['steps']):
            body = step.get('run', '')
            name = step.get('name', body or step.get('uses', 'step')).splitlines()[0]
            row = dict(name=name, index=index, status='not run')
            job['steps'].append(row)
            action = step.get('uses', '')
            if action and body:
                row['reason'] = 'mixed action and command body'
                continue
            if action.startswith(('actions/checkout@', 'actions/cache/', 'actions/upload-artifact@')):
                row.update(status='local', reason='private source/cache/logs replace GitHub transport')
                continue
            if action.startswith('actions/setup-python@'):
                version = str(step.get('with', {}).get('python-version', ''))
                row.update(status='local' if version == '3.13.15' else 'not run',
                           reason='private verified Python3.13.15' if version == '3.13.15' else 'unsupported Python version')
                continue
            if action.startswith('actions/setup-node@'):
                version = str(step.get('with', {}).get('node-version', ''))
                row.update(status='local' if version == '24' else 'not run',
                           reason='private verified Node24' if version == '24' else 'unsupported Node version')
                continue
            if action:
                row['reason'] = 'unsupported action'
                continue
            if body.rstrip('\n') in TOOLCHAINS:
                row.update(status='local', reason='verified pinned private Rust toolchain; no host default changes')
                continue
            if body.rstrip('\n') in {CI.RUST_CACHE.format(toolchain=t) for t in ('', ' +1.99.1')}:
                row.update(status='local', reason='exclusive local cache; no GitHub cache key')
                continue
            unsafe = ('outside-job.py', 'test_windows_installer_survival', 'ensure-webview2.py',
                      'check-windows-portable.py', 'scripts/ci/client-native.sh windows core', 'scripts/ci/windows-next-shell.sh')
            if any(marker in body for marker in unsafe):
                row['reason'] = 'owner-machine exclusion: hosted/breakaway, installer/runtime, or shared desktop effect; body not executed'
                continue
            condition = step.get('if', '')
            if condition not in ('', 'always()', '${{ !cancelled() }}', 'failure()'):
                row['reason'] = 'unsupported condition: ' + condition
                continue
            shell = step.get('shell', defaults.get('shell', 'bash'))
            if shell != 'bash' or not body or '${{' in body:
                row['reason'] = 'unsupported shell/body/expression'
                continue
            variables = {**source.get('env', {}), **step.get('env', {})}
            for name, value in list(variables.items()):
                if value == '${{ inputs.windows_installer_version }}':
                    variables[name] = ''
            if any(('${{' in str(v) and v != '${{ github.workspace }}/target/windows-next') or name.startswith('HGS_') or name == 'ZERUS_CLIENT_DESKTOP_SERVICE'
                   for name, v in variables.items()):
                row['reason'] = 'unsupported or authority-bearing environment'
                continue
            row.update(body=body, body_sha256=CI.digest(body.encode()), directory=step.get('working-directory', '.'),
                       env=variables, shell=shell, condition=condition,
                       timeout=min(1800, int(step.get('timeout-minutes', source.get('timeout-minutes', 30))) * 60))
    return jobs



def producer_ready(producer, commit, target):
    """A prior successful build is useful only while these exact bytes remain."""
    return bool(producer and producer.get('commit') == commit and producer.get('files') and
                all((target / name).is_file() and CI.file_digest(target / name) == digest
                    for name, digest in producer['files'].items()))



def close_gate(process):
    if process is not None and process.stdin is not None and not process.stdin.closed:
        try:
            process.stdin.close()
        except BrokenPipeError:
            pass


def settle_original_worker(process):
    """Settle this Popen's original handle, including pre-assignment refusal."""
    if process is None:
        return
    try:
        close_gate(process)
    except OSError:
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)

def immutable_checkout(repository, commit, destination, environment):
    """Materialize only Git objects; the caller's index/worktree are not input."""
    subprocess.run(['git', '-c', 'core.autocrlf=false', '-c', 'core.hooksPath=/dev/null',
                    'clone', '--shared', '--no-checkout', str(repository), str(destination)],
                   env=environment, check=True, stdout=subprocess.DEVNULL, timeout=120)
    subprocess.run(['git', '-C', str(destination), '-c', 'core.autocrlf=false',
                    '-c', 'core.hooksPath=/dev/null', 'checkout', '--detach', commit],
                   env=environment, check=True, stdout=subprocess.DEVNULL, timeout=120)
    if CI.git(destination, 'rev-parse', 'HEAD') != commit:
        raise RuntimeError('private checkout does not match selected commit')

def vc_environment(root):
    vswhere = Path(os.environ['ProgramFiles(x86)']) / 'Microsoft Visual Studio/Installer/vswhere.exe'
    selected = subprocess.check_output([str(vswhere), '-latest', '-products', '*', '-requires',
                                       'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'], text=True).strip()
    script = Path(selected) / 'Common7/Tools/VsDevCmd.bat'
    batch = root / 'compiler-environment.cmd'
    batch.write_text('@echo off\ncall "' + str(script) + '" -arch=x64 -host_arch=x64 >nul\nif errorlevel 1 exit /b 1\nset\n')
    result = subprocess.check_output(['cmd.exe', '/d', '/c', str(batch)],
                                     env={**os.environ, 'HOME': str(root), 'USERPROFILE': str(root),
                                          'TEMP': str(root / 'tmp'), 'TMP': str(root / 'tmp'), 'VSCMD_SKIP_SENDTELEMETRY': '1'},
                                     text=True, errors='replace', timeout=60)
    allowed = {'PATH', 'INCLUDE', 'LIB', 'LIBPATH', 'VCTOOLSINSTALLDIR', 'VCINSTALLDIR', 'VSINSTALLDIR',
               'WINDOWSSDKDIR', 'WINDOWSSDKVERSION', 'UCRTVERSION', 'UNIVERSALCRTSDKDIR', 'VSCMD_ARG_TGT_ARCH', 'VSCMD_ARG_HOST_ARCH'}
    return {key.upper(): value for line in result.splitlines() if '=' in line
            for key, value in [line.split('=', 1)] if key.upper() in allowed}


class Runner:
    def __init__(self, args, report):
        self.args, self.report = args, report
        self.root, self.output = args.root.resolve(), args.output.resolve()
        self.target = self.root / 'target'
        self.python = self.root / 'tools/python-nuget/tools/python.exe'
        self.node = self.root / 'tools/node' / args.node_directory
        self.cargo = self.root / 'cargo'
        self.bash = Path('C:/Program Files/Git/bin/bash.exe')
        self.until = datetime.datetime.fromisoformat(args.until.replace('Z', '+00:00'))

    def prepare(self):
        if self.output.exists() or not self.output.is_relative_to(self.root):
            raise RuntimeError('output must be new and under the private root')
        self.output.mkdir(parents=True)
        self.target.mkdir(exist_ok=True)
        self.sdk = vc_environment(self.root)
        self.report['tool_versions'] = {}
        for name, command in {'python': [str(self.python), '--version'], 'node': [str(self.node / 'node.exe'), '--version'],
                              'rust': [str(self.cargo / 'bin/rustc.exe'), '+1.99.1', '-Vv']}.items():
            result = subprocess.run(command, env=self.environment(self.output / 'probe-home'), capture_output=True, text=True, timeout=30)
            self.report['tool_versions'][name] = dict(exit_code=result.returncode, version=result.stdout.strip())
            if result.returncode:
                raise RuntimeError('private tool failed: ' + name)
        if not re.fullmatch(r'v24\.[0-9]+\.[0-9]+', self.report['tool_versions']['node']['version']):
            raise RuntimeError('wrong private Node major; Node24 required')
        if self.report['tool_versions']['python']['version'] != 'Python 3.13.15':
            raise RuntimeError('wrong private Python version')
        self.snapshot = self.output / 'snapshot'
        immutable_checkout(self.args.repo, self.report['commit'], self.snapshot,
                           self.environment(self.output / 'source-home'))
        spec = importlib.util.spec_from_file_location('owned_job', self.snapshot / 'scripts/ci/outside-job.py')
        self.custody = importlib.util.module_from_spec(spec); spec.loader.exec_module(self.custody)
        self.report['custody_source_sha256'] = CI.file_digest(self.snapshot / 'scripts/ci/outside-job.py')
        self.report.update(workers=4, min_free_gib=10, slot_until=self.args.until)
        harness = self.output / 'harness'; harness.mkdir()
        for path in Path(__file__).parent.glob('local*.py'):
            shutil.copy2(path, harness / path.name)
        self.report['harness_files'] = {p.name: CI.file_digest(p) for p in harness.iterdir()}

    def environment(self, home):
        home.mkdir(parents=True, exist_ok=True)
        temporary = home / 'tmp';temporary.mkdir(exist_ok=True)
        paths = [self.cargo / 'bin', self.python.parent, self.python.parent / 'Scripts', self.node,
                 'C:/Program Files/Git/cmd']
        paths += [p for p in self.sdk.get('PATH', '').split(';')
                  if p.lower().replace('/', '\\').startswith((
                      'c:\\windows', 'c:\\program files\\microsoft visual studio',
                      'c:\\program files (x86)\\windows kits', 'c:\\program files (x86)\\microsoft visual studio'))]
        paths += ['C:/Windows/System32', 'C:/Windows', 'C:/Strawberry/c/bin', 'C:/Program Files/Git/usr/bin']
        env = {name: value for name, value in os.environ.items() if name.upper() in
               {'SYSTEMROOT', 'WINDIR', 'COMSPEC', 'PATHEXT', 'SYSTEMDRIVE', 'PROGRAMFILES', 'PROGRAMFILES(X86)', 'PROGRAMDATA', 'COMPUTERNAME'}}
        env.update(self.sdk)
        env.update(HOME=str(home), USERPROFILE=str(home), APPDATA=str(home / 'AppData/Roaming'),
                   LOCALAPPDATA=str(home / 'AppData/Local'), TEMP=str(temporary), TMP=str(temporary),
                   PATH=';'.join(map(str, paths)), CARGO_HOME=str(self.cargo), RUSTUP_HOME=str(self.root / 'rustup'),
                   RUSTUP_TOOLCHAIN='1.99.1', CARGO_TARGET_DIR=str(self.target), CARGO_BUILD_JOBS='4',
                   CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0',
                   RUST_TEST_THREADS='4', RAYON_NUM_THREADS='4', ZERUS_PYTHON_JOBS='4', ZERUS_TEST_JOBS='4',
                   PYTHONNOUSERSITE='1', PYTHONDONTWRITEBYTECODE='1', PYTHONUTF8='1',
                   GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=str(home / '.gitconfig'),
                   GIT_TERMINAL_PROMPT='0', GCM_INTERACTIVE='never', VSCMD_SKIP_SENDTELEMETRY='1',
                   NPM_CONFIG_USERCONFIG=str(home / '.npmrc'), NPM_CONFIG_CACHE=str(home / '.npm-cache'),
                   CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=str(Path(self.sdk['VCTOOLSINSTALLDIR']) / 'bin/Hostx64/x64/link.exe'))
        return env

    def run_job(self, job):
        workspace = self.output / 'checkouts' / job['id'];workspace.parent.mkdir(exist_ok=True)
        immutable_checkout(self.snapshot, self.report['commit'], workspace,
                           self.environment(self.output / 'homes' / job['id']))
        subprocess.run(['cmd.exe', '/d', '/c', 'mklink', '/J', str(workspace / 'target'), str(self.target)], check=True,
                       stdout=subprocess.DEVNULL, env=self.environment(self.output / 'homes' / job['id']))
        job['checkout'] = str(workspace)
        home = self.output / 'homes' / job['id'];env = self.environment(home)
        producers = {}
        env.update(GITHUB_WORKSPACE=str(workspace), RUNNER_TEMP=str(home / 'runner-temp'),
                   GITHUB_OUTPUT=str(home / 'github-output'), GITHUB_ENV=str(home / 'github-env'), GITHUB_PATH=str(home / 'github-path'))
        Path(env['RUNNER_TEMP']).mkdir(exist_ok=True)
        for step in job['steps']:
            if 'body' not in step:
                continue
            if (self.root / 'STOP').exists() or datetime.datetime.now(datetime.timezone.utc) >= self.until:
                step.update(status='not run', reason='slot pause/return deadline; no new command')
                continue
            if shutil.disk_usage(self.root).free < 10 * 1024**3:
                step.update(status='not run', reason='C: 10 GiB floor; no new command')
                continue
            requirements = {'Windows client integration tests': 'cli',
                            'Per-user installer from the portable zip': 'portable'}
            if step['name'] in requirements and not producer_ready(producers.get(requirements[step['name']]),
                                                                  self.report['commit'], self.target):
                step.update(status='not run', reason='exact-source producer did not pass; cached image not adopted')
                continue
            directory = Path(step['directory'])
            if directory.is_absolute() or '..' in directory.parts:
                step.update(status='not run', reason='working directory escapes source')
                continue
            script = self.output / (job['id'] + '-' + str(step['index']) + '.sh')
            script.write_text('set -euo pipefail\nmkdir -p artifacts/test-results\n' + step['body'], newline='\n')
            log = script.with_suffix('.log')
            environment = {**env, **{k: str(workspace / 'target/windows-next')
                                      if v == '${{ github.workspace }}/target/windows-next' else str(v)
                                      for k, v in step['env'].items()}}
            command = [str(self.bash), '-e', '-o', 'pipefail', str(script)]
            spec = script.with_suffix('.json')
            # The original owned worker cannot fork before its handle is in our
            # non-breakaway kill-on-close job. No path/PID wildcard cleanup.
            spec.write_text(json.dumps(dict(argv=command, env=environment)))
            job_handle = self.custody.create_job()
            process = None
            step.update(status='running', started_at=time.time(), log=log.name)
            CI.save(self.output / 'report.json', self.report)
            print('RUN', job['id'], step['name'], flush=True)
            try:
                with log.open('wb') as stream:
                    process = subprocess.Popen([str(self.python), '-I', str(self.snapshot / 'scripts/ci/outside-job.py'), '--action', str(spec)],
                                               cwd=workspace / directory, env=environment, stdin=subprocess.PIPE,
                                               stdout=stream, stderr=subprocess.STDOUT)
                    self.custody.assign_job(job_handle, int(process._handle))
                    process.stdin.write(b'G');process.stdin.close()
                    deadline = time.monotonic() + step['timeout']
                    while process.poll() is None:
                        if (self.root / 'STOP').exists() or datetime.datetime.now(datetime.timezone.utc) >= self.until:
                            raise RuntimeError('slot pause/return; original private job stopped')
                        if time.monotonic() >= deadline or shutil.disk_usage(self.root).free < 10 * 1024**3:
                            raise RuntimeError('step deadline or C: floor; original private job stopped')
                        time.sleep(1)
                    code = process.returncode
                step.update(status='pass' if code == 0 else 'fail', exit_code=code)
                if code == 0 and step['name'] == 'Release CLI and connector for host and package checks':
                    leaves = ['release/hgs.exe', 'release/zerus-mobile-connector.exe',
                              'debug/examples/fake_ssh.exe', 'debug/examples/fake_agent.exe']
                    producer = dict(commit=self.report['commit'], files={name: CI.file_digest(self.target / name) for name in leaves})
                    producers['cli'] = producer
                    step['local_producer'] = producer
            except (RuntimeError, OSError) as error:
                step.update(status='fail', reason=str(error))
            finally:
                try:
                    close_gate(process)
                finally:
                    try:
                        self.custody.close_job(job_handle)
                    finally:
                        settle_original_worker(process)
                step.update(seconds=round(time.time() - step['started_at'], 2), log_sha256=CI.file_digest(log))
                job['status'] = CI.summarize(job['steps']);CI.save(self.output / 'report.json', self.report)
            print(step['status'].upper(), job['id'], step['name'], step['seconds'], flush=True)
        job['status'] = CI.summarize(job['steps'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True);parser.add_argument('--repo', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True);parser.add_argument('--sha', required=True)
    parser.add_argument('--node-directory', required=True);parser.add_argument('--until', required=True)
    parser.add_argument('--jobs')
    args = parser.parse_args()
    if os.name != 'nt':parser.error('requires Windows')
    commit = CI.resolve(args.repo, args.sha)
    workflow_bytes = subprocess.check_output(['git', '-C', str(args.repo), 'show', commit + ':' + CI.WORKFLOW])
    report = dict(version=1, commit=commit, platform='native Windows owner-machine preflight', completed=False,
                  workflow_sha256=CI.digest(workflow_bytes), runner_sha256=CI.file_digest(Path(__file__)),
                  jobs=plan(CI.load_workflow(workflow_bytes), set(args.jobs.split(',')) if args.jobs else None))
    runner = Runner(args, report)
    try:
        runner.prepare()
        order = {'windows-tests': 0, 'client-native-windows': 1, 'windows-desktop': 2, 'windows-host': 3}
        for job in sorted(report['jobs'], key=lambda j: order.get(j['id'], 9)):
            if any('body' in step for step in job['steps']):runner.run_job(job)
        report['completed'] = True
    except Exception as error:
        report['preparation_or_runner_error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        if runner.output.exists():CI.save(runner.output / 'report.json', report)
    print('Report:', runner.output / 'report.json')
    return 1 if any(j['status'] == 'fail' for j in report['jobs']) else 2


if __name__ == '__main__':sys.exit(main())
