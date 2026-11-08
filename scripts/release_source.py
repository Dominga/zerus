"""Bind a parallel source-recipe qualification to the exact final release archive."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

from release_common import require, sha256


def source_archive(root, version, destination):
    """No filename or wall-clock timestamp enters the gzip header."""
    epoch = int(subprocess.check_output(['git', 'show', '-s', '--format=%ct', 'HEAD'], cwd=root, text=True))
    archive = subprocess.check_output(['git', 'archive', '--format=tar', f'--prefix=zerus-{version}/', 'HEAD'], cwd=root)
    with destination.open('xb') as output, gzip.GzipFile(filename='', fileobj=output, mode='wb', compresslevel=9, mtime=epoch) as zipped:
        zipped.write(archive)
    return hashlib.sha256(archive).hexdigest()


def source_recipe(root, version, archive):
    return (root / 'packaging/aur/zerus/PKGBUILD.in').read_text().replace('@VERSION@', version).replace('@SOURCE_SHA256@', sha256(archive))


def identity(root, version):
    require(re.fullmatch(r'\d+\.\d+\.\d+', version) and (root / 'VERSION').read_text().strip() == version,
            'Source qualification version must match VERSION.')
    require(not subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=normal'], cwd=root, text=True).strip(),
            'Source qualification requires a clean committed checkout.')
    return subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()


def prepare(root, directory, version):
    commit = identity(root, version)
    directory.mkdir(parents=True, exist_ok=True)
    require(not any(directory.iterdir()), 'Use an empty source qualification directory.')
    archive = directory / f'zerus-{version}-source.tar.gz'
    tar_hash = source_archive(root, version, archive)
    recipe = directory / 'aur/zerus/PKGBUILD'
    recipe.parent.mkdir(parents=True)
    recipe.write_text(source_recipe(root, version, archive))
    (directory / 'release-info.json').write_text(json.dumps(dict(version=version, source_commit=commit, tar_sha256=tar_hash)) + '\n')


def record(root, directory):
    """Called only after makepkg (including check), namcap and check-package pass."""
    info = json.loads((directory / 'release-info.json').read_text())
    version = info['version']
    require(identity(root, version) == info['source_commit'], 'Source qualification changed commit.')
    names = [f'zerus-{version}-source.tar.gz', 'aur/zerus/PKGBUILD', 'aur/zerus/.SRCINFO',
             f'zerus-{version}-1-x86_64.pkg.tar.zst']
    hashes = {name: sha256(directory / name) for name in names}
    require((directory / names[1]).read_text() == source_recipe(root, version, directory / names[0]), 'Source recipe changed.')
    receipt = dict(schema=1, version=version, source_commit=info['source_commit'], tar_sha256=info['tar_sha256'], files=hashes)
    with (directory / 'source-check.json').open('x') as output:
        output.write(json.dumps(receipt, indent=2) + '\n')


def reuse(root, qualified, candidate, version, commit):
    receipt_path = qualified / 'source-check.json'
    require(receipt_path.stat().st_size <= 4096, 'Source qualification receipt exceeds its bound.')
    receipt = json.loads(receipt_path.read_text())
    require(set(receipt) == {'schema', 'version', 'source_commit', 'tar_sha256', 'files'} and receipt['schema'] == 1,
            'Invalid source qualification receipt.')
    require(receipt['version'] == version and receipt['source_commit'] == commit, 'Source qualification belongs to another version or commit.')
    archive = f'zerus-{version}-source.tar.gz'
    package = f'zerus-{version}-1-x86_64.pkg.tar.zst'
    names = {archive, package, 'aur/zerus/PKGBUILD', 'aur/zerus/.SRCINFO'}
    require(set(receipt['files']) == names, 'Source qualification inventory differs.')
    for name in names:
        path = qualified / name
        require(path.is_file() and not path.is_symlink() and sha256(path) == receipt['files'][name], 'Source qualification bytes changed.')
    # Rebuild the archive independently in the final job, rather than trusting
    # a digest supplied by the early job without comparing the final bytes.
    require(sha256(candidate / archive) == receipt['files'][archive] and
            hashlib.sha256(gzip.decompress((candidate / archive).read_bytes())).hexdigest() == receipt['tar_sha256'],
            'Final git archive differs from the qualified source archive.')
    recipe = candidate / 'aur/zerus/PKGBUILD'
    require(sha256(recipe) == receipt['files']['aur/zerus/PKGBUILD'], 'Final source recipe differs from the qualified recipe.')
    srcinfo = subprocess.check_output(['makepkg', '--printsrcinfo'], cwd=recipe.parent)
    require(hashlib.sha256(srcinfo).hexdigest() == receipt['files']['aur/zerus/.SRCINFO'], 'Final source metadata differs from the qualified metadata.')
    (recipe.parent / '.SRCINFO').write_bytes(srcinfo)
    shutil.copyfile(qualified / package, candidate / package)
    print('Final git archive SHA-256 equals the independently qualified archive: ' + receipt['files'][archive])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('prepare', 'record'))
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--version')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if args.operation == 'prepare':
        prepare(root, args.directory, args.version or '')
    else:
        record(root, args.directory)


if __name__ == '__main__':
    main()
