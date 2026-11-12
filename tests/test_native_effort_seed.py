"""Private seed provenance controls; no Rust compiler, seed or native agent runs."""
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('native_effort_seed',ROOT/'scripts/ci/native-effort-seed.py')
seed=importlib.util.module_from_spec(spec)
spec.loader.exec_module(seed)


class Fixture:
    def __init__(self,root):
        self.root=root
        subprocess.run(['git','init','-q',str(root)],env=seed.git_environment(),check=True)
        (root/'.gitignore').write_text('target/\nartifacts/\n')
        subprocess.run(['git','add','.gitignore'],cwd=root,env=seed.git_environment(),check=True)
        subprocess.run(['git','-c','user.name=Fixture','-c','user.email=fixture@example.test',
                        'commit','-qm','Synthetic source'],cwd=root,env=seed.git_environment(),check=True)
        self.bound=seed.source(root)
        self.hgs=root/'target/debug/hgs';self.hgs.parent.mkdir(parents=True)
        self.image=self.hgs.with_name('hgs-effort-seed')
        self.receipt=self.hgs.with_name('hgs-effort-seed.json')
        self.hgs.write_bytes(b'synthetic-cli');self.image.write_bytes(b'synthetic-test-image')
        self.image.chmod(0o500)
        self.value=seed.write_receipt(root,self.hgs,self.image,self.receipt,self.bound['commit'],'fixture Rust 1.99.1')


class NativeEffortSeed(unittest.TestCase):
    def fixture(self):
        temporary=tempfile.TemporaryDirectory(prefix='seed-proof-')
        self.addCleanup(temporary.cleanup)
        return Fixture(Path(temporary.name))

    def test_same_clean_source_cli_and_private_image_are_bound_without_execution(self):
        f=self.fixture()
        real_run=subprocess.run
        def metadata_only(argv,**options):
            self.assertEqual(argv[0],'git','only source metadata may run')
            return real_run(argv,**options)
        with patch.object(seed.subprocess,'run',side_effect=metadata_only):
            self.assertEqual(seed.verify(f.root,f.receipt,f.hgs),f.value)
        self.assertEqual(seed.environment(f.value),dict(ZERUS_PICKER_EFFORT_SEED_IMAGE=str(f.image),
            ZERUS_PICKER_EFFORT_SEED_SHA256=hashlib.sha256(f.image.read_bytes()).hexdigest()))
        self.assertEqual(f.image.stat().st_mode&0o777,0o500)
        self.assertEqual(f.receipt.stat().st_mode&0o777,0o600)

    def test_dirty_changed_or_untracked_source_refuses_before_compilation(self):
        f=self.fixture()
        with patch.dict(os.environ,GIT_DIR='/unreviewed/repository',GIT_WORK_TREE='/unreviewed/tree',GIT_INDEX_FILE='/unreviewed/index'):
            self.assertEqual(seed.source(f.root),f.bound)
        with self.assertRaisesRegex(ValueError,'selected source'):seed.source(f.root,'f'*40)
        for file in (f.root/'.gitignore',f.root/'untracked.rs'):
            previous=file.read_bytes() if file.exists() else None
            file.write_text('unreviewed change')
            with self.assertRaisesRegex(ValueError,'clean source'):seed.verify(f.root,f.receipt,f.hgs)
            if previous is None:file.unlink()
            else:file.write_bytes(previous)
        self.assertEqual(seed.verify(f.root,f.receipt,f.hgs),f.value)

    def test_changed_image_cli_and_writable_seed_refuse(self):
        f=self.fixture()
        for path in (f.hgs,f.image):
            original=path.read_bytes();mode=path.stat().st_mode&0o777
            path.chmod(0o700);path.write_bytes(original+b'changed');path.chmod(mode)
            with self.assertRaisesRegex(ValueError,'digest'):seed.verify(f.root,f.receipt,f.hgs)
            path.chmod(0o700);path.write_bytes(original);path.chmod(mode)
        f.image.chmod(0o700)
        with self.assertRaisesRegex(ValueError,'writable'):seed.verify(f.root,f.receipt,f.hgs)

    def test_strict_receipt_source_method_duplicate_keys_and_aliases_refuse(self):
        f=self.fixture()
        for change in (lambda v:v['source'].update(tree='f'*40),lambda v:v.update(method='arbitrary_test'),
                       lambda v:v.update(extra=True),lambda v:v['image'].update(bytes=True)):
            altered=copy.deepcopy(f.value);change(altered);f.receipt.write_text(json.dumps(altered))
            with self.assertRaises(ValueError):seed.verify(f.root,f.receipt,f.hgs)
        f.receipt.write_text('{"schema":1,'+json.dumps(f.value)[1:])
        with self.assertRaisesRegex(ValueError,'Duplicate'):seed.verify(f.root,f.receipt,f.hgs)
        f.receipt.write_text(json.dumps(f.value))
        alias=f.image.with_name('seed-alias');alias.symlink_to(f.image)
        with self.assertRaises(OSError):seed.identity(alias)
        alias.unlink();alias.symlink_to(f.receipt)
        with self.assertRaises(OSError):seed.verify(f.root,alias,f.hgs)

    def test_normal_current_debug_sizes_fit_but_oversize_refuses_before_read(self):
        with tempfile.TemporaryDirectory(prefix='seed-size-') as temporary:
            path=Path(temporary)/'sparse'
            # Real current root artifacts, represented without allocating a
            # second GiB cache. Hashing remains streaming and bounded.
            for size in (594433472,754488872):
                with path.open('wb') as stream:stream.truncate(size)
                self.assertLess(path.stat().st_blocks*512,1024*1024)
                self.assertEqual(seed.identity(path)['bytes'],size)
            with path.open('wb') as stream:stream.truncate(seed.MAX_BINARY+1)
            with patch.object(seed.hashlib,'sha256',side_effect=AssertionError('must refuse before read')):
                with self.assertRaises(ValueError):seed.identity(path)

    def test_cargo_selects_exactly_one_root_profile_artifact(self):
        row=dict(reason='compiler-artifact',target=dict(name='hgs',kind=['bin']),profile=dict(test=True),executable='/fixture/hgs-test')
        encode=lambda rows:io.BytesIO(b''.join(json.dumps(item).encode()+b'\n' for item in rows))
        self.assertEqual(seed.cargo_image(encode([row])),Path('/fixture/hgs-test'))
        for rows in ([],[row,row],[dict(row,target=dict(name='other',kind=['bin']))],
                     [dict(row,profile=dict(test=False))],[dict(row,executable=None)]):
            with self.assertRaises(ValueError):seed.cargo_image(encode(rows))
        with self.assertRaises(ValueError):seed.cargo_image(io.BytesIO(b'x'*(1024*1024+1)))

    def test_builder_uses_one_same_tree_cli_and_test_profile_and_lists_only_ignored_seed(self):
        f=self.fixture();commands=[]
        origin=f.root/'target/debug/deps/hgs-unit';origin.parent.mkdir();origin.write_bytes(b'compiled unit fixture')
        original_check_output=subprocess.check_output
        real_run=subprocess.run
        def run(argv,**options):
            if argv[0]=='git':return real_run(argv,**options)
            commands.append(argv)
            if argv[0]=='cargo':
                is_test=argv[1]=='test'
                row=dict(reason='compiler-artifact',target=dict(name='hgs',kind=['bin']),profile=dict(test=is_test),
                         executable=str(origin if is_test else f.hgs))
                options['stdout'].write((json.dumps(row)+'\n').encode())
                return SimpleNamespace(returncode=0)
            self.assertEqual(argv,[str(origin),'--ignored','--list','--exact',seed.METHOD])
            return SimpleNamespace(returncode=0,stdout=(seed.METHOD+': test\n\n1 test, 0 benchmarks\n').encode(),stderr=b'')
        args=SimpleNamespace(source_sha=f.bound['commit'],hgs=f.hgs,image=f.image,receipt=f.receipt)
        with patch.object(seed,'ROOT',f.root),patch.object(seed.subprocess,'run',side_effect=run), \
             patch.object(seed.subprocess,'check_output',side_effect=lambda argv,**kw:'fixture Rust identity' if argv==['rustc','-Vv'] else original_check_output(argv,**kw)):
            seed.build(args)
        self.assertEqual(len(commands),3)
        self.assertIn('--no-run',commands[1]);self.assertIn('--locked',commands[0]);self.assertIn('--locked',commands[1])
        value=seed.verify(f.root,f.receipt,f.hgs)
        self.assertEqual(value['image']['sha256'],hashlib.sha256(origin.read_bytes()).hexdigest())
