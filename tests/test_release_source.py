"""Parallel source qualification must bind real Git archive bytes, not a label."""
import gzip
import io
import tarfile
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import release_source as source


class SourceQualificationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='zerus-source-qualification-')
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.root = self.base / 'early'
        self.root.mkdir()
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        (self.root / 'VERSION').write_text('0.99.0\n')
        (self.root / '.gitignore').write_text('output/\n')
        (self.root / '.gitattributes').write_text('omitted.txt export-ignore\n')
        (self.root / 'omitted.txt').write_text('not shipped\n')
        (self.root / 'payload').write_text('exact checked source\n')
        template = self.root / 'packaging/aur/zerus/PKGBUILD.in'
        template.parent.mkdir(parents=True)
        template.write_bytes((ROOT / 'packaging/aur/zerus/PKGBUILD.in').read_bytes())
        subprocess.run(['git', 'add', '.'], cwd=self.root, check=True)
        subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                        'commit', '-qm', 'source fixture'], cwd=self.root, check=True,
                       env={**os.environ, 'GIT_AUTHOR_DATE': '2026-01-01T00:00:00Z', 'GIT_COMMITTER_DATE': '2026-01-01T00:00:00Z'})
        self.final = self.base / 'final'
        subprocess.run(['git', 'clone', '-q', '--no-local', str(self.root), str(self.final)], check=True)
        self.early = self.root / 'output'
        self.candidate = self.final / 'output'
        source.prepare(self.root, self.early, '0.99.0')
        source.prepare(self.final, self.candidate, '0.99.0')
        self.commit = source.identity(self.root, '0.99.0')
        self.package = 'zerus-0.99.0-1-x86_64.pkg.tar.zst'
        (self.early / self.package).write_bytes(b'checked package')
        self.srcinfo = b'pkgbase = zerus\n\tpkgver = 0.99.0\n'
        (self.early / 'aur/zerus/.SRCINFO').write_bytes(self.srcinfo)
        source.record(self.root, self.early)

    def reuse(self):
        with patch.object(source.subprocess, 'check_output', return_value=self.srcinfo):
            source.reuse(self.final, self.early, self.candidate, '0.99.0', self.commit)

    def test_git_archive_is_byte_identical_in_two_independent_checkouts(self):
        a = (self.early / 'zerus-0.99.0-source.tar.gz').read_bytes()
        b = (self.candidate / 'zerus-0.99.0-source.tar.gz').read_bytes()
        self.assertEqual(a, b)
        self.assertEqual(hashlib.sha256(gzip.decompress(a)).hexdigest(),
                         json.loads((self.early / 'source-check.json').read_text())['tar_sha256'])
        with tarfile.open(fileobj=io.BytesIO(a), mode='r:gz') as archive:
            self.assertNotIn('zerus-0.99.0/omitted.txt', archive.getnames())
        self.reuse()
        self.assertEqual((self.candidate / self.package).read_bytes(), b'checked package')

    def test_an_archive_rebuilt_from_different_source_cannot_reuse_the_package(self):
        path = self.candidate / 'zerus-0.99.0-source.tar.gz'
        path.write_bytes(gzip.compress(b'another source', mtime=0))
        with self.assertRaisesRegex(ValueError, 'Final git archive differs'):
            self.reuse()
        self.assertFalse((self.candidate / self.package).exists())

    def test_equal_tar_with_a_different_gzip_header_is_still_refused(self):
        path = self.candidate / 'zerus-0.99.0-source.tar.gz'
        path.write_bytes(gzip.compress(gzip.decompress(path.read_bytes()), mtime=0))
        with self.assertRaisesRegex(ValueError, 'Final git archive differs'):
            self.reuse()
        self.assertFalse((self.candidate / self.package).exists())

    def test_wrong_sha_and_tampered_qualification_are_not_accepted(self):
        receipt = self.early / 'source-check.json'
        original = receipt.read_bytes()
        for key, value in [('source_commit', 'b' * 40), ('version', '0.98.0'), ('tar_sha256', '0' * 64)]:
            with self.subTest(key=key):
                data = json.loads(original); data[key] = value; receipt.write_text(json.dumps(data))
                with self.assertRaises(ValueError): self.reuse()
                self.assertFalse((self.candidate / self.package).exists())
        receipt.write_bytes(original)
        for name in [self.package, 'aur/zerus/PKGBUILD', 'aur/zerus/.SRCINFO']:
            with self.subTest(file=name):
                path = self.early / name; old = path.read_bytes(); path.write_bytes(old + b'changed')
                with self.assertRaisesRegex(ValueError, 'bytes changed'): self.reuse()
                path.write_bytes(old)

    def test_final_recipe_and_generated_srcinfo_must_still_match(self):
        recipe = self.candidate / 'aur/zerus/PKGBUILD'
        original = recipe.read_bytes(); recipe.write_bytes(original + b'changed')
        with self.assertRaisesRegex(ValueError, 'Final source recipe differs'): self.reuse()
        recipe.write_bytes(original)
        self.srcinfo += b'changed'
        with self.assertRaisesRegex(ValueError, 'Final source metadata differs'): self.reuse()
        self.assertFalse((self.candidate / self.package).exists())

    def test_no_receipt_is_not_success_and_output_cannot_be_overwritten(self):
        (self.early / 'source-check.json').unlink()
        with self.assertRaises(FileNotFoundError): self.reuse()
        with self.assertRaisesRegex(ValueError, 'empty'): source.prepare(self.root, self.early, '0.99.0')


if __name__ == '__main__':
    unittest.main()
