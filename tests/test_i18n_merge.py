"""Task 272: the Next catalog merge driver.

`client/src/i18n.next-ru.json` merges as a key union (a key both sides changed
differently stays a conflict with both values); `client/src/i18n.ru.json`, the
catalog generated from it, merges the same way and is written sorted, as the
generator writes it (task #22). The cases run the real driver as Git invokes
it, with three-way fixtures, and one real `git merge`.
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

    def test_a_sorted_supplement_stays_sorted(self):
        # Task #22 (team ede724c4): the checked-in supplement is sorted and the
        # generator's --check requires it; theirs' new keys no longer trail.
        result, text = driver(({"b": "2"}, {"b": "2", "c": "3"}, {"a": "1", "b": "2"}))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(list(json.loads(text)), ["a", "b", "c"])

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
    def test_the_generated_catalog_merges_both_sides_keys_sorted(self):
        # Task #22 (team ede724c4): each side's catalog was generated from its
        # own supplement; the merge keeps both sides' keys, sorted as the
        # generator writes them.
        base = {"a": "1", "m": "2"}
        ours = {"a": "1", "m": "2", "z": "ours"}
        theirs = {"a": "1", "b": "theirs", "m": "2"}
        result, text = driver((base, ours, theirs), path=GENERATED)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(text, json.dumps({"a": "1", "b": "theirs", "m": "2", "z": "ours"}, ensure_ascii=False, indent=2, sort_keys=True) + "\n")
        # A key both sides changed differently stays a conflict, never one side.
        result, text = driver(({"k": "base"}, {"k": "ours"}, {"k": "theirs"}), path=GENERATED)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('"k": "ours"', text)
        self.assertIn('"k": "theirs"', text)

    def test_a_real_merge_keeps_the_strings_both_branches_added(self):
        # The bug: the driver regenerated the catalog from the supplement in
        # the working tree, which Git writes only after every driver ran, so
        # the merged catalog lost the keys the other branch added (merge
        # 276e77248 dropped task #7's strings).
        with tempfile.TemporaryDirectory(prefix="zerus-i18n-repo-") as temporary:
            repo = Path(temporary)

            def git(*arguments):
                done = subprocess.run(
                    ["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@example.com",
                     "-c", "commit.gpgsign=false", *arguments],
                    capture_output=True, text=True, check=False)
                self.assertEqual(done.returncode, 0, done.stderr)
                return done.stdout

            def write(relative, catalog):
                path = repo / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(catalog, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n")

            git("init", "-q")
            (repo / ".gitattributes").write_text(f"{SUPPLEMENT} merge=i18n\n{GENERATED} merge=i18n\n", encoding="utf-8")
            write(SUPPLEMENT, {"base": "б"})
            write(GENERATED, {"base": "б", "qt": "кт"})
            git("add", "-A")
            git("commit", "-qm", "base")
            main = git("rev-parse", "--abbrev-ref", "HEAD").strip()
            git("checkout", "-qb", "theirs")
            write(SUPPLEMENT, {"base": "б", "theirs": "их"})
            write(GENERATED, {"base": "б", "qt": "кт", "theirs": "их"})
            git("commit", "-qam", "theirs")
            git("checkout", "-q", main)
            write(SUPPLEMENT, {"base": "б", "ours": "наши"})
            write(GENERATED, {"base": "б", "ours": "наши", "qt": "кт"})
            git("commit", "-qam", "ours")
            command = f"{Path(sys.executable).as_posix()} {SCRIPT.as_posix()} %O %A %B %L %P"
            git("-c", f"merge.i18n.driver={command}", "merge", "-q", "--no-edit", "theirs")
            merged = json.loads((repo / GENERATED).read_text(encoding="utf-8"))
            self.assertEqual(merged, {"base": "б", "ours": "наши", "qt": "кт", "theirs": "их"})
            self.assertEqual(json.loads((repo / SUPPLEMENT).read_text(encoding="utf-8")),
                             {"base": "б", "ours": "наши", "theirs": "их"})

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
