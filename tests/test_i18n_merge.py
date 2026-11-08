"""Task 272: the Next catalog merge driver.

`client/src/i18n.next-ru.json` merges as a key union (a key both sides changed
differently stays a conflict with both values); `client/src/i18n.ru.json` is
regenerated from the merged supplement instead of being merged. The cases run
the real driver as Git invokes it, with three-way fixtures.
"""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/i18n-merge.py"
SUPPLEMENT = "client/src/i18n.next-ru.json"
GENERATED = "client/src/i18n.ru.json"
spec = importlib.util.spec_from_file_location("i18n_merge", SCRIPT)
merge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(merge)


def driver(files, path=SUPPLEMENT):
    """Run the driver as Git does: `%O %A %B %L %P`, the result left in ours."""
    with tempfile.TemporaryDirectory(prefix="zerus-i18n-") as temporary:
        base = Path(temporary) / "base.json"
        ours = Path(temporary) / "ours.json"
        theirs = Path(temporary) / "theirs.json"
        for target, value in ((base, files[0]), (ours, files[1]), (theirs, files[2])):
            if isinstance(value, dict):
                value = json.dumps(value, ensure_ascii=False, indent=2) + "\n"
            target.write_text(value, encoding="utf-8", newline="\n")
        result = subprocess.run(
            [sys.executable, str(SCRIPT), str(base), str(ours), str(theirs), "7", path],
            capture_output=True, text=True, encoding="utf-8", check=False)
        return result, ours.read_text(encoding="utf-8")


class SupplementMergeTests(unittest.TestCase):
    def test_disjoint_key_additions_merge_as_a_union(self):
        result, text = driver(({"shared": "s"}, {"shared": "s", "one": "1"},
                               {"shared": "s", "two": "2"}))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(text), {"shared": "s", "one": "1", "two": "2"})
        # Ours' order stays; theirs' new keys follow.
        self.assertEqual(list(json.loads(text)), ["shared", "one", "two"])
        self.assertEqual(text, json.dumps(json.loads(text), ensure_ascii=False, indent=2) + "\n")

    def test_the_same_key_with_the_same_text_is_one_entry(self):
        result, text = driver(({}, {"new": "same"}, {"new": "same"}))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(text), {"new": "same"})
        self.assertEqual(text.count('"new"'), 1)

    def test_the_same_key_with_different_texts_stays_a_conflict_with_both_values(self):
        result, text = driver(({}, {"key": "ours text"}, {"key": "theirs text"}))
        self.assertEqual(result.returncode, 1, result.stderr)
        for part in ("<<<<<<< ours", "=======", ">>>>>>> theirs", '"key": "ours text"', '"key": "theirs text"'):
            self.assertIn(part, text)
        self.assertIn("both values", result.stderr)

    def test_plural_lists_keep_their_three_forms(self):
        forms = ["%n вещь", "%n вещи", "%n вещей"]
        result, text = driver(({}, {"%n items": forms}, {"other": "x"}))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(text)["%n items"], forms)
        # Different forms on both sides is a visible conflict, lists included.
        result, text = driver(({}, {"%n items": forms}, {"%n items": ["%n a", "%n b", "%n c"]}))
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('"%n вещь"', text)
        self.assertIn('"%n a"', text)

    def test_a_deletion_on_one_side_merges_and_a_deletion_against_a_change_conflicts(self):
        result, text = driver(({"gone": "1", "kept": "2"}, {"kept": "2"}, {"kept": "2"}))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(text), {"kept": "2"})
        result, text = driver(({"key": "base"}, {}, {"key": "changed"}))
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('"key": "changed"', text)

    def test_another_path_is_not_an_i18n_catalog(self):
        result, _ = driver(({}, {}, {}), path="client/src/other.json")
        self.assertEqual(result.returncode, 2)
        self.assertIn("no i18n merge rule", result.stderr)


class GeneratedCatalogTests(unittest.TestCase):
    def test_the_generated_catalog_is_regenerated_from_the_supplement(self):
        # The driver ignores both sides of i18n.ru.json and rebuilds it from the
        # checked-in supplement and shared catalog.
        stale = json.dumps({"stale": "must not survive"}, ensure_ascii=False, indent=2) + "\n"
        result, text = driver((stale, stale, stale), path=GENERATED)
        self.assertEqual(result.returncode, 0, result.stderr)
        checked_in = (ROOT / GENERATED).read_text(encoding="utf-8")
        self.assertEqual(text.replace("\r\n", "\n"), checked_in.replace("\r\n", "\n"))

    def test_regeneration_uses_a_merged_supplement(self):
        checked = merge.loaded(ROOT / SUPPLEMENT)
        supplement = dict(checked)
        supplement["driver test key"] = "значение водителя"
        generated = merge.regenerate(supplement, None)
        self.assertEqual(json.loads(generated)["driver test key"], "значение водителя")
        # Every checked-in entry survives the round trip.
        first = next(iter(checked))
        self.assertEqual(json.loads(generated)[first], checked[first])


class RegistrationTests(unittest.TestCase):
    def test_gitattributes_registers_the_driver_for_both_catalogs(self):
        attributes = (ROOT / ".gitattributes").read_text(encoding="utf-8")
        for path in (SUPPLEMENT, GENERATED):
            self.assertIn(f"{path} merge=i18n\n", attributes)

    def test_the_contributor_setup_names_the_driver_and_the_script(self):
        contributing = (ROOT / "CONTRIBUTING.md").read_text(encoding="utf-8")
        self.assertIn("merge.i18n.driver", contributing)
        self.assertIn("scripts/i18n-merge.py", contributing)
        self.assertIn("Zerus", contributing)


if __name__ == "__main__":
    unittest.main()
