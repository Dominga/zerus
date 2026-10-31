#!/usr/bin/env python3
"""U62: measure each test target of one map provider on its own and compare
what it covered with .zerus/test-map.json (hgs test coverage).

cargo providers run each module target under cargo-llvm-cov; ctest providers
run each test binary's CTest entries in a build configured with gcc's
--coverage and read the counters with gcovr. The differences go to the job
summary and to OUT/<provider>.json; the script fails only when it cannot
measure, never because of a gap: a gap is a finding for the map.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def safe(name):
    # Readable, and unique even where two ids clean to the same text.
    readable = re.sub(r'[^A-Za-z0-9_.-]+', '_', name).strip('_.-')[:80] or 'root'
    return readable + '-' + hashlib.sha1(name.encode()).hexdigest()[:8]


def run(command, cwd, log):
    with open(log, 'a', encoding='utf-8') as out:
        out.write('$ ' + ' '.join(command) + '\n')
        out.flush()
        return subprocess.run(command, cwd=cwd, stdout=out, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL).returncode


def cargo_reports(provider, section, settings, out, log):
    cwd = ROOT / settings.get('root', '')
    reports = {}
    for target_id, target in sorted(section['targets'].items()):
        report = out / (safe(target_id) + '.info')
        run(['cargo', 'llvm-cov', 'clean', '--workspace'], cwd, log)
        # One module alone: its filters, its children skipped.
        filters = list(target.get('run', []))
        for skip in target.get('skip', []):
            filters += ['--skip', skip]
        group = target.get('group', '').split()
        status = run(['cargo', 'llvm-cov', '--no-report', 'test', '--locked', *group, '--', *filters], cwd, log)
        if run(['cargo', 'llvm-cov', 'report', '--lcov', '--output-path', str(report)], cwd, log) == 0:
            reports[target_id] = report
        if status != 0:
            print(f'{provider}: {target_id} failed its tests; its coverage still counts', file=sys.stderr)
    return reports


def ctest_reports(provider, section, settings, out, log, build):
    reports = {}
    for target_id, target in sorted(section['targets'].items()):
        for counters in build.rglob('*.gcda'):
            counters.unlink()
        names = '|'.join(re.escape(name) for name in target.get('run', []))
        if not names:
            continue
        status = run(['ctest', '--test-dir', str(build), '-R', f'^({names})$', '--output-on-failure'], ROOT, log)
        if status != 0 and not any(build.rglob('*.gcda')):
            print(f'{provider}: {target_id} did not run here', file=sys.stderr)
            continue
        report = out / (safe(target_id) + '.xml')
        if run(['gcovr', '--root', str(ROOT), '--cobertura', str(report), str(build)], ROOT, log) == 0:
            reports[target_id] = report
    return reports


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('provider')
    parser.add_argument('--out', default='artifacts/coverage')
    parser.add_argument('--build', help='ctest: the build folder configured with --coverage')
    parser.add_argument('--hgs', default=str(ROOT / 'target' / 'debug' / 'hgs'))
    parser.add_argument('--limit', type=int, default=0, help='measure only the first N targets')
    args = parser.parse_args()
    out = (ROOT / args.out / args.provider).resolve()
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    log = out / 'runs.log'
    config = json.loads(subprocess.check_output([args.hgs, 'test', 'config', '--json'], cwd=ROOT, text=True))
    settings = next((p for p in config['loaded']['config'].get('provider', []) if p['name'] == args.provider), None)
    if settings is None:
        raise SystemExit(f'No provider {args.provider} in .zerus/tests.toml')
    test_map = json.loads((ROOT / config['loaded']['config']['map']['file']).read_text(encoding='utf-8'))
    section = test_map['providers'][args.provider]
    if args.limit:
        section = dict(section, targets=dict(sorted(section['targets'].items())[:args.limit]))
    if settings['kind'] == 'cargo':
        reports = cargo_reports(args.provider, section, settings, out, log)
    elif settings['kind'] == 'ctest':
        if not args.build:
            raise SystemExit('A ctest provider needs --build')
        reports = ctest_reports(args.provider, section, settings, out, log, Path(args.build).resolve())
    else:
        raise SystemExit(f'No coverage runner for {settings["kind"]} providers yet')
    if not reports:
        raise SystemExit(f'{args.provider}: no coverage was measured; see {log}')
    command = [args.hgs, 'test', 'coverage', '--provider', args.provider, '--json']
    for target_id, report in reports.items():
        command += ['--report', f'{target_id}={report}']
    result = json.loads(subprocess.check_output(command, cwd=ROOT, text=True))
    (out / f'{args.provider}.json').write_text(json.dumps(result, indent=1) + '\n', encoding='utf-8')
    lines = [f'## Test map against coverage: {args.provider}', '',
             f'{len(reports)} target(s) measured, {result["gaps"]} gap(s) (covered, not in the map).', '']
    for item in result['targets']:
        if item['gaps']:
            lines.append(f'- `{item["target"]}` covers {len(item["gaps"])} file(s) the map does not give it: '
                         + ', '.join(f'`{gap}`' for gap in item['gaps'][:20]))
    unused = sum(len(item['unused']) for item in result['targets'])
    lines += ['', f'{unused} mapped file(s) a target never executed (over-selection, for review).', '']
    # #718, the monthly report: tests that cover nothing no other test covers.
    redundant = result.get('without_unique_coverage', [])
    lines += [f'{len(redundant)} target(s) without unique coverage (candidates to merge or drop): '
              + ', '.join(f'{target}' for target in redundant[:40]), '']
    summary = os.environ.get('GITHUB_STEP_SUMMARY')
    text = '\n'.join(lines) + '\n'
    if summary:
        with open(summary, 'a', encoding='utf-8') as handle:
            handle.write(text)
    print(text)


if __name__ == '__main__':
    main()
