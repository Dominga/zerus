#!/usr/bin/env python3
"""Build/seal one private Rust test image; never execute its ignored seed.

The maintained native fixtures alone run the closed seed against their own
existing record. All Effort requests still use the production hgs executable.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
METHOD = 'state::effort::picker_scope::tests::effort::seed_actual_effort_record'
MAX_BINARY = 1024 * 1024**2
MAX_RECEIPT = 32 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def git_environment():
    env={key:value for key,value in os.environ.items() if not key.startswith('GIT_')}
    env.update(GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL=os.devnull,GIT_OPTIONAL_LOCKS='0')
    return env


def source(root, expected=None):
    def git(*args):
        return subprocess.check_output(['git','-c','core.fsmonitor=false','-c','core.excludesfile='+os.devnull,*args],
                                       cwd=root,env=git_environment(),text=True,timeout=10).strip()
    require(Path(git('rev-parse','--show-toplevel')).resolve()==Path(root).resolve(),
            'Seed source must be the selected repository root')
    commit, tree = git('rev-parse', 'HEAD'), git('rev-parse', 'HEAD^{tree}')
    require(re.fullmatch('[0-9a-f]{40}', commit) and re.fullmatch('[0-9a-f]{40}', tree),
            'An exact committed source tree is required')
    require(expected is None or commit == expected, 'Seed source differs from selected source')
    require(not git('status', '--porcelain', '--untracked-files=normal'),
            'Seed build/consumption requires the exact clean source tree')
    return dict(commit=commit, tree=tree)


def identity(path, allow_linked=False):
    """One regular no-follow leaf, bounded and unchanged throughout hashing."""
    flags = os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0)
    with os.fdopen(os.open(path, flags), 'rb') as stream:
        before = os.fstat(stream.fileno())
        require(stat.S_ISREG(before.st_mode) and 0 < before.st_size <= MAX_BINARY
                and (allow_linked or before.st_nlink == 1), 'Invalid native seed/CLI leaf')
        digest, size = hashlib.sha256(), 0
        while chunk := stream.read(1024 * 1024):
            size += len(chunk)
            require(size <= before.st_size, 'Native seed/CLI grew while hashing')
            digest.update(chunk)
        after = os.fstat(stream.fileno())
        require(size == before.st_size and
                (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) ==
                (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns),
                'Native seed/CLI changed while hashing')
    return dict(bytes=size, sha256=digest.hexdigest())


def write_receipt(root, hgs, image, path, expected_source, rust, pins=None):
    """Called by this builder or the already-verified macOS artifact installer."""
    bound = source(root, expected_source)
    hgs, image, path = map(lambda p: Path(p).absolute(), (hgs, image, path))
    require(hgs != image, 'A seed must never replace the production CLI')
    actual = dict(hgs=identity(hgs, allow_linked=True), image=identity(image))
    require(pins is None or actual == pins, 'Verified handoff changed before seed receipt')
    require(isinstance(rust, str) and 0 < len(rust) <= 16384, 'Unknown producing Rust identity')
    value = dict(schema=1, kind='native-effort-seed', source=bound, method=METHOD,
                 rust_sha256=hashlib.sha256(rust.encode()).hexdigest(),
                 hgs=dict(path=str(hgs), **actual['hgs']), image=dict(path=str(image), **actual['image']))
    raw = (json.dumps(value, sort_keys=True) + '\n').encode()
    require(len(raw) <= MAX_RECEIPT, 'Seed receipt exceeds its bound')
    path.parent.mkdir(parents=True, exist_ok=True)
    require(not path.is_symlink() and (not path.exists() or path.is_file()), 'Unknown seed receipt destination')
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.effort-seed-', delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
            os.fchmod(stream.fileno(), 0o600)
            require(source(root, expected_source) == bound, 'Source changed before seed receipt publication')
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)
    return value


def verify(root, receipt, hgs):
    flags=os.O_RDONLY|getattr(os,'O_NOFOLLOW',0)|getattr(os,'O_NONBLOCK',0)
    with os.fdopen(os.open(receipt,flags),'rb') as stream:
        before=os.fstat(stream.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_nlink==1 and 0<before.st_size<=MAX_RECEIPT,
                'Invalid seed receipt leaf')
        raw = stream.read(MAX_RECEIPT + 1)
        after=os.fstat(stream.fileno())
        require((before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)==
                (after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns)
                and len(raw)==before.st_size,'Seed receipt changed while reading')
    require(0 < len(raw) <= MAX_RECEIPT, 'Invalid seed receipt size')
    def distinct(pairs):
        result={}
        for key,value in pairs:
            require(key not in result,'Duplicate seed receipt key')
            result[key]=value
        return result
    value = json.loads(raw,object_pairs_hook=distinct)
    require(set(value) == {'schema', 'kind', 'source', 'method', 'rust_sha256', 'hgs', 'image'}
            and type(value['schema']) is int and value['schema'] == 1
            and value['kind'] == 'native-effort-seed' and value['method'] == METHOD,
            'Unknown native seed receipt')
    require(value['source'] == source(root), 'Seed receipt source/tree differs')
    require(isinstance(value['rust_sha256'],str) and re.fullmatch('[0-9a-f]{64}', value['rust_sha256']), 'Unknown seed Rust identity')
    for key in ('hgs', 'image'):
        record = value[key]
        require(set(record) == {'path', 'bytes', 'sha256'} and type(record['bytes']) is int
                and isinstance(record['path'], str) and Path(record['path']).is_absolute()
                and isinstance(record['sha256'], str) and re.fullmatch('[0-9a-f]{64}', record['sha256']),
                'Invalid seed input binding')
        require(identity(Path(record['path']), allow_linked=(key == 'hgs')) ==
                {name: record[name] for name in ('bytes', 'sha256')}, 'Native seed/CLI digest differs')
    require(Path(value['hgs']['path']).resolve(strict=True) == Path(hgs).resolve(strict=True),
            'Seed receipt binds another CLI path')
    require(Path(value['image']['path']).stat().st_mode&0o777==0o500,
            'Private seed image is writable or not private/executable')
    return value


def environment(value):
    return dict(ZERUS_PICKER_EFFORT_SEED_IMAGE=value['image']['path'],
                ZERUS_PICKER_EFFORT_SEED_SHA256=value['image']['sha256'])


def cargo_image(stream, test=True):
    images = []
    total = 0
    while raw := stream.readline(1024 * 1024 + 1):
        total += len(raw)
        require(len(raw) <= 1024 * 1024 and total <= 64 * 1024**2, 'Cargo artifact output exceeds its bound')
        row = json.loads(raw)
        if (row.get('reason') == 'compiler-artifact' and row.get('target', {}).get('name') == 'hgs'
                and row.get('target', {}).get('kind') == ['bin'] and row.get('profile', {}).get('test') is test
                and row.get('executable')):
            images.append(Path(row['executable']))
    require(len(images) == 1, 'Expected exactly one Cargo-reported hgs executable for this profile')
    return images[0]


def build(args):
    bound = source(ROOT, args.source_sha)
    require(args.image.name == 'hgs-effort-seed' and args.receipt.name == 'hgs-effort-seed.json',
            'Use the fixed private seed image/receipt filenames')
    require(args.hgs.resolve() != args.image.resolve(), 'A seed must never replace the production CLI')
    rust = subprocess.check_output(['rustc', '-Vv'], text=True, timeout=10)
    with tempfile.TemporaryFile() as output:
        subprocess.run(['cargo', 'build', '--locked', '--bin', 'hgs', '--message-format=json', '-j', '2'],
                       cwd=ROOT,env=git_environment(),stdout=output,check=True,timeout=1200)
        output.seek(0)
        cli = cargo_image(output, test=False)
    require(cli.resolve(strict=True) == args.hgs.resolve(strict=True), 'CLI path differs from its Cargo build artifact')
    cli_pin = identity(cli, allow_linked=True)
    with tempfile.TemporaryFile() as output:
        subprocess.run(['cargo', 'test', '--locked', '--bin', 'hgs', '--no-run', '--message-format=json', '-j', '2'],
                       cwd=ROOT,env=git_environment(),stdout=output,check=True,timeout=1200)
        output.seek(0)
        image = cargo_image(output)
    require(source(ROOT, args.source_sha) == bound, 'Source changed during seed compilation')
    expected = identity(image, allow_linked=True)
    listed = subprocess.run([str(image), '--ignored', '--list', '--exact', METHOD], cwd=ROOT,
                            stdin=subprocess.DEVNULL, capture_output=True, check=True, timeout=10)
    require(len(listed.stdout) <= 16384 and [line for line in listed.stdout.decode().splitlines() if line] ==
            [METHOD + ': test', '1 test, 0 benchmarks'], 'The exact ignored seed method is missing')
    require(identity(image, allow_linked=True) == expected, 'Cargo test image changed after listing')
    args.image.parent.mkdir(parents=True, exist_ok=True)
    require(not args.image.is_symlink() and (not args.image.exists() or args.image.is_file()),
            'Unknown private seed destination')
    with tempfile.NamedTemporaryFile(dir=args.image.parent, prefix='.effort-seed-', delete=False) as output:
        temporary = Path(output.name)
        try:
            with image.open('rb') as incoming:
                while chunk := incoming.read(1024 * 1024):
                    output.write(chunk)
            output.flush()
            os.fsync(output.fileno())
            require(identity(temporary) == expected and identity(image, allow_linked=True) == expected,
                    'Private seed copy differs from the compiled original')
            os.fchmod(output.fileno(), 0o500)
            os.replace(temporary, args.image)
        finally:
            temporary.unlink(missing_ok=True)
    write_receipt(ROOT, args.hgs, args.image, args.receipt, args.source_sha, rust,
                  pins=dict(hgs=cli_pin, image=expected))
    verify(ROOT, args.receipt, args.hgs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('build',))
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--hgs', type=Path, required=True)
    parser.add_argument('--image', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    build(args)


if __name__ == '__main__':
    main()
