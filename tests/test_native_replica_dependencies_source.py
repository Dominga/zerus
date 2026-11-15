"""Native build caches and the remote source bundle cover Cargo's local closure."""
from pathlib import Path
import re
import shlex
import tomllib
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def replica_crates(root=ROOT):
    pending = [root / 'crates/replica-ffi']
    found = set()
    while pending:
        crate = pending.pop().resolve()
        if crate in found:
            continue
        found.add(crate)
        manifest = tomllib.loads((crate / 'Cargo.toml').read_text())
        # Cargo resolves manifests for dev/build and every target table even
        # when the selected command only builds the production library.
        sections = [manifest, *manifest.get('target', {}).values()]
        tables = [section.get(kind, {}) for section in sections
                  for kind in ('dependencies', 'dev-dependencies', 'build-dependencies')]
        for dependencies in tables:
            for dependency in dependencies.values():
                if isinstance(dependency, dict) and 'path' in dependency:
                    pending.append(crate / dependency['path'])
    return {crate.relative_to(root).as_posix() for crate in found}


class NativeReplicaDependencies(unittest.TestCase):
    def test_resolution_walk_includes_dev_build_target_and_transitive_paths(self):
        with tempfile.TemporaryDirectory(prefix='zerus-native-closure-') as folder:
            root = Path(folder)
            start = root / 'crates/replica-ffi'
            start.mkdir(parents=True)
            (start / 'Cargo.toml').write_text('''[package]
name = "fixture"
version = "0.1.0"
[dependencies]
normal = { path = "../../native/normal" }
[dev-dependencies]
dev = { path = "../../native/dev" }
[build-dependencies]
build = { path = "../../native/build" }
[target.'cfg(unix)'.dependencies]
target_normal = { path = "../../native/target-normal" }
[target.'cfg(windows)'.dev-dependencies]
target_dev = { path = "../../native/target-dev" }
[target.'cfg(target_os = "macos")'.build-dependencies]
target_build = { path = "../../native/target-build" }
''')
            leaves = {'normal', 'dev', 'build', 'target-normal', 'target-dev', 'target-build', 'nested'}
            for name in leaves:
                crate = root / 'native' / name
                crate.mkdir(parents=True)
                text = f'[package]\nname = "{name}"\nversion = "0.1.0"\n'
                if name == 'dev':
                    text += '[build-dependencies]\nnested = { path = "../nested" }\n'
                (crate / 'Cargo.toml').write_text(text)
            self.assertEqual(replica_crates(root), {'crates/replica-ffi'} | {'native/' + name for name in leaves})

    def test_android_task_inputs_cover_every_local_native_crate(self):
        source = (ROOT / 'mobile/android/app/build.gradle.kts').read_text()
        declaration = re.search(r'val replicaSources = (.*?)\.flatMap', source, re.S).group(1)
        ffi = re.search(r'val replicaCrate = rootProject.file\("([^"]+)"\)', source).group(1)
        inputs = re.findall(r'rootProject.file\("([^"]+)"\)', declaration)
        if 'replicaCrate' in declaration:
            inputs.append(ffi)
        declared = {
            (ROOT / 'mobile/android' / path).resolve().relative_to(ROOT).as_posix()
            for path in inputs
        }
        self.assertFalse(replica_crates() - declared, 'Cargo dependency is absent from Gradle inputs')

    def test_ios_fingerprint_covers_each_manifest_and_source_directory(self):
        source = (ROOT / 'mobile/ios/scripts/build-replica.sh').read_text()
        stamp = source.split('stamp=$(', 1)[1].split('\nstamp_file=', 1)[0]
        paths = set(re.findall(r'(?:crates|services)/[A-Za-z0-9_./-]+', stamp))
        required = {crate + suffix for crate in replica_crates() for suffix in ['/Cargo.toml', '/src']}
        self.assertFalse(required - paths, 'Cargo dependency can change without changing the cache stamp')

    def test_remote_bundle_includes_every_local_native_crate(self):
        source = (ROOT / 'mobile/ios/scripts/remote-dev-install.sh').read_text()
        rsync = source.split('\nrsync ', 1)[1].split('\n\n', 1)[0]
        tokens = shlex.split(rsync.replace('\\\n', ' '))
        includes = {tokens[i + 1] for i, token in enumerate(tokens[:-1]) if token == '--include'}
        required = {'/' + crate + suffix for crate in replica_crates() for suffix in ['/', '/**']}
        self.assertFalse(required - includes, 'A native Cargo dependency is missing from the upload allowlist')

    def test_remote_revision_marks_changes_to_every_local_native_crate(self):
        source = (ROOT / 'mobile/ios/scripts/remote-dev-install.sh').read_text()
        line = next(line for line in source.splitlines() if 'status --porcelain --' in line)
        paths = shlex.split(line.split('status --porcelain --', 1)[1].split(')', 1)[0])
        missing = {crate for crate in replica_crates() if not any(crate == path or crate.startswith(path + '/') for path in paths)}
        self.assertFalse(missing, 'Dirty native sources would be reported as a clean revision')


if __name__ == '__main__':
    unittest.main()
