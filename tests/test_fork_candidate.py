"""Fork identities and bounded unsigned candidate inventories; no executables run."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import fork_identity as identity
import package_fork_candidate as bundle

COMMIT = "a" * 40
VERSION = "2026.10.25-1"


def cli(os):
    return {"schema_version":1,"product":"hgs","version":"1.56.0","distribution":"romanilyin/zerus",
            "fork_version":VERSION,"channel":"candidate","source_commit":COMMIT,"source_dirty":False,
            "os":os,"architecture":"x86_64","wsl_receiver_abi":"wsl-local-v1","path_ref_schema":1,
            "opencode_binding_abi":"opencode-bind-existing-v1"}


def gui():
    return {"version":"0.47.0","distribution":"romanilyin/zerus","forkVersion":VERSION,"forkChannel":"candidate",
            "sourceCommit":COMMIT,"sourceDirty":False,"forkUpdatesEnabled":False}


def windows():
    return {"distribution":"romanilyin/zerus","fork_version":VERSION,"fork_channel":"candidate","source_commit":COMMIT,
            "cli_build":cli("windows"),"gui_build":gui()}


class ForkCandidate(unittest.TestCase):
    def test_matching_clean_components_keep_one_source_identity(self):
        bundle.validate_pair(windows(),cli("linux"),COMMIT,VERSION)

    def test_foreign_dirty_or_mismatched_binaries_are_refused(self):
        for key,value in [("schema_version",True),("distribution","ufna/zerus"),("source_commit","b"*40),
                          ("source_dirty",True),("source_dirty",0),("fork_version","2026.10.24-1"),("channel","stable"),
                          ("os","windows"),("architecture","aarch64"),("wsl_receiver_abi","unknown"),
                          ("path_ref_schema",True),("opencode_binding_abi","unknown")]:
            candidate=cli("linux");candidate[key]=value
            with self.subTest(key=key,value=value),self.assertRaises(ValueError):
                bundle.validate_pair(windows(),candidate,COMMIT,VERSION)
        for key,value in [("sourceDirty",True),("forkUpdatesEnabled",True),("sourceCommit","b"*40),("forkChannel","stable")]:
            w=windows();w["gui_build"][key]=value
            with self.subTest(key=key),self.assertRaises(ValueError):bundle.validate_pair(w,cli("linux"),COMMIT,VERSION)

    def test_version_and_artifact_paths_are_strict(self):
        for version in ["2026.2.30-1","2026.10.25-0","2026.10.25-1\n","bad","2026.10.25-"+"1"*100]:
            with self.subTest(version=version),self.assertRaises(ValueError):identity.fork_version(version)
        for path in ["../outside","/absolute","C:/outside","linux//file","linux/../file","linux/CON.txt","linux/bad ","linux/a\\b"]:
            with self.subTest(path=path),self.assertRaises(ValueError):bundle.safe_relative(path)

    def make(self, root):
        for name,data in {"windows/zerus-windows-x86_64.zip":b"fixture-archive","linux/bin/hgs":b"fixture-elf", "linux/bin/hgs_state.py":b"fixture-compatibility-shim",
                          "linux/receiver.json":json.dumps(cli("linux")).encode(),"linux/licenses/LICENSE":b"fixture-license"}.items():
            p=root/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
        rows=[{"path":p.relative_to(root).as_posix(),"bytes":p.stat().st_size,"sha256":bundle.digest(p)}
              for p in sorted(root.rglob("*")) if p.is_file()]
        value={"schema_version":1,"distribution":"romanilyin/zerus","fork_version":VERSION,"channel":"candidate",
               "source_commit":COMMIT,"signed":False,"published":False,"files":rows}
        (root/"fork-candidate.json").write_text(json.dumps(value))
        return value

    def test_inventory_verification_is_data_only_and_detects_tampering(self):
        with tempfile.TemporaryDirectory() as temp,patch.object(bundle.windows_portable,"inspect",return_value=windows()),\
             patch.object(bundle.subprocess,"check_output",side_effect=AssertionError("verification must not execute")):
            root=Path(temp);self.make(root);bundle.verify(root)
            (root/"linux/bin/hgs").write_bytes(b"changed")
            with self.assertRaises(ValueError):bundle.verify(root)

    def test_unlisted_nested_manifest_and_duplicate_paths_fail(self):
        with tempfile.TemporaryDirectory() as temp,patch.object(bundle.windows_portable,"inspect",return_value=windows()):
            root=Path(temp);value=self.make(root)
            (root/"linux/fork-candidate.json").write_text('{}')
            with self.assertRaises(ValueError):bundle.verify(root)
            (root/"linux/fork-candidate.json").unlink()
            value["files"].append(value["files"][0]);(root/"fork-candidate.json").write_text(json.dumps(value))
            with self.assertRaises(ValueError):bundle.verify(root)

    @unittest.skipIf(sys.platform=="win32","symlink fixture requires Unix permissions")
    def test_symlinked_parent_is_refused_before_reading_an_outside_file(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)/'root';root.mkdir();outside=Path(temp)/'outside';outside.mkdir();(outside/'file').write_text('private-fixture')
            (root/'link').symlink_to(outside,target_is_directory=True)
            with self.assertRaises(ValueError):bundle.contained_file(root,bundle.safe_relative('link/file'))

    def test_candidate_cannot_claim_publication_or_signature(self):
        with tempfile.TemporaryDirectory() as temp,patch.object(bundle.windows_portable,"inspect",return_value=windows()):
            root=Path(temp);original=self.make(root)
            for field in ['signed','published']:
                value=copy.deepcopy(original);value[field]=True;(root/'fork-candidate.json').write_text(json.dumps(value))
                with self.assertRaises(ValueError):bundle.verify(root)


if __name__=='__main__':unittest.main()
