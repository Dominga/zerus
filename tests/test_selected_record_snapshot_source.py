"""Reviewed consumers and read-only inventory of the selected-record snapshot."""
import importlib.util
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("portable_source", ROOT / "scripts/ci/check-source.py")
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class SelectedRecordSnapshotSource(unittest.TestCase):
    def test_snapshot_has_only_the_reviewed_fresh_resume_and_fork_consumers(self):
        references = {
            str(path.relative_to(ROOT))
            for path in (ROOT / "src").rglob("*.rs")
            if re.search(r"\bselected_source\b", CHECK.code_only(path.read_text()))
        }
        self.assertEqual(references, {
            "src/state/storage.rs",
            "src/state/storage/selected_source_tests.rs",
            "src/state/storage/selected_source_tests/cases.rs",
            "src/state/lifecycle.rs",
            "src/state/fork/source.rs",
        })
        storage = CHECK.code_only((ROOT / "src/state/storage.rs").read_text())
        self.assertEqual(len(re.findall(r"\bselected_source\b", storage)), 1,
                         "no production re-export or additional wiring in the declaring module")
        self.assertIn("pub(super) mod selected_source;", storage)
        lifecycle = CHECK.code_only((ROOT / "src/state/lifecycle.rs").read_text())
        fresh = lifecycle.split("pub(super) fn fresh(", 1)[1].split("fn verify_requested(", 1)[0]
        resume = lifecycle.split("pub(super) fn resume_scoped(", 1)[1].split("pub(super) fn fresh(", 1)[0]
        self.assertEqual(len(re.findall(r"\bselected_source\b", lifecycle)), 2)
        self.assertIn("let record = source.record();", fresh)
        self.assertIn("let current = read(name).map_err(|_| changed())?;", fresh)
        self.assertEqual(fresh.count("record_path(name)"), 1, "resolve once before capture")
        self.assertLess(fresh.index("lock(None)?"), fresh.index("source.recheck()"))
        self.assertLess(fresh.index("source.recheck()"), fresh.index("backend::current().start"))
        self.assertIn("let record = source.record();", resume)
        self.assertIn("source.into_fence()", resume)
        self.assertIn("let current = read(&name).map_err(|_| changed())?;", resume)
        self.assertLess(resume.index("lock(None)?"), resume.index("source.recheck()"))
        self.assertLess(resume.index("source.recheck()"), resume.index("scope.changing()"))

    def test_fork_source_checks_cover_both_effects_without_refreshing_the_proof(self):
        code = CHECK.code_only((ROOT / "src/state/fork.rs").read_text())
        body = code.split("pub(super) fn start_scoped(", 1)[1].split("#[cfg(test)]", 1)[0]
        self.assertLess(body.index("lock(None)?"), body.index("source::Selected::capture(name)"))
        self.assertIn("source.as_ref().map(source::Selected::record)", body)
        self.assertLess(body.index("source.check(name)?"), body.index("kimi_fork(record)"))
        self.assertLess(body.index("source.map(source::Selected::into_fence)"), body.rindex("source.check(name)"))
        self.assertLess(body.rindex("source.check(name)"), body.index("backend::current().start"))
        self.assertIn("source::kept(error, new_id.as_deref().filter(|_| !dry))", body)
        proof = CHECK.code_only((ROOT / "src/state/fork/source.rs").read_text())
        conversion = proof.split("fn into_fence(self) -> Fence {", 1)[1].split("\n    }", 1)[0]
        self.assertNotRegex(conversion, r"\b(?:read|capture|check|namespace|clone)\s*\(")
        self.assertIn("self.source.into_fence()", conversion)

    def test_snapshot_uses_only_the_resolved_metadata_reader(self):
        code = CHECK.code_only((ROOT / "src/state/storage/selected_source.rs").read_text())
        self.assertNotRegex(code, r"\b(?:read|read_metadata|record_path|label_record_path|records|"
                                 r"write|atomic|private_dir|lock|reconcile|Command|backend)\s*(?:\(|::)")
        self.assertNotRegex(code, r"\b(?:Serialize|Deserialize|Drop)\b")
        self.assertIn("read_metadata_at(slot, name)", code)
        self.assertIn("read_metadata_at(&self.slot, &self.name)", code)

    def test_consuming_fence_conversion_never_refreshes_or_keeps_the_raw_value(self):
        code = CHECK.code_only((ROOT / "src/state/storage/selected_source.rs").read_text())
        conversion = code.split("fn into_fence(self) -> Fence {", 1)[1].split("}", 1)[0]
        self.assertEqual(conversion.strip(), "self.fence", "move original proof; no read, re-hash, copy or leak")


if __name__ == "__main__":
    unittest.main()
