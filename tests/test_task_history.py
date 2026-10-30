"""The optional paged get beside the legacy full-journal task contract."""
import hashlib
import json
import uuid
from test_tasks import Fixture, agent_record


class TaskHistory(Fixture):
    def test_journal_only_edits_change_the_streamed_listing_revision(self):
        task = self.add("Canonical description")
        def listed():
            return self.hgs("list", "--project", "orchard")["tasks"][0]
        before = listed()
        self.assertEqual(before["history_revision"], listed()["history_revision"])
        self.hgs("comment", task["uuid"], "--project", "orchard", text="External note")
        after = listed()
        self.assertNotEqual(before.pop("history_revision"), after.pop("history_revision"))
        self.assertEqual(before, after, "a comment changes no canonical list field")
        self.assertNotIn("history_revision", self.hgs("get", task["uuid"], "--project", "orchard")["task"])

    def test_bounded_pages_match_the_unchanged_legacy_journal(self):
        task = self.add("Canonical description")
        for n in range(6):
            self.hgs("comment", task["uuid"], "--project", "orchard", text=f"Note {n}")
        legacy = self.hgs("get", task["uuid"], "--project", "orchard")["task"]
        before = None
        log = []
        while True:
            args = ("--history-before", before) if before else ()
            paged = self.hgs("get", task["uuid"], "--project", "orchard",
                             "--history-limit", "2", *args)["task"]
            self.assertEqual(paged["history_schema"], 1)
            self.assertLessEqual(len(paged["log"]), 2)
            self.assertEqual(paged["log_total"], len(legacy["log"]))
            self.assertEqual(paged["text"], legacy["text"])
            self.assertEqual(paged["dispatch_id"], legacy["dispatch_id"])
            log = paged["log"] + log
            before = paged["history_next"]
            if before is None:
                break
        self.assertEqual(log, legacy["log"])
        self.assertEqual(self.hgs("get", task["uuid"], "--project", "orchard")["task"], legacy)
        self.assertNotIn("history_schema", legacy)

    def test_new_notes_keep_an_older_page_and_cursors_are_scoped(self):
        task = self.add("First task")
        for n in range(5):
            self.hgs("comment", task["uuid"], "--project", "orchard", text=f"Note {n}")
        def page(*args):
            return self.hgs("get", task["uuid"], "--project", "orchard", "--history-limit", "2", *args)["task"]
        cursor = page()["history_next"]
        older = page("--history-before", cursor)
        self.hgs("comment", task["uuid"], "--project", "orchard", text="New note")
        self.assertEqual(page("--history-before", cursor)["log"], older["log"])
        other = self.add("Another task")
        refused = self.hgs("get", other["uuid"], "--project", "orchard", "--history-limit", "2",
                           "--history-before", cursor, ok=False)
        self.assertIn("cursor does not match this task", refused)
        self.assertEqual(page("--history-before", cursor)["log"], older["log"])

    def test_invalid_pages_fail_instead_of_silently_returning_a_full_journal(self):
        task = self.add("Example")
        for limit in ["0", "81", "no"]:
            self.assertIn("history limit", self.hgs("get", task["uuid"], "--project", "orchard",
                "--history-limit", limit, ok=False))
        self.assertIn("requires --history-limit", self.hgs("get", task["uuid"], "--project", "orchard",
            "--history-before", "cursor", ok=False))

    def test_cached_metadata_resolves_a_renamed_session_again(self):
        old = agent_record(self.state, self.env, "codex/example", "example-run", launch_id=str(uuid.uuid4()))
        task = self.add("Bound executor", "--target", "codex/example")
        args = ("get", task["uuid"], "--project", "orchard", "--history-limit", "2")
        self.assertEqual(self.hgs(*args)["task"]["target"]["session"], "codex/example")
        record = json.loads(old.read_text())
        old.unlink()
        record["name"] = "codex/renamed"
        record["journal_name"] = "codex/example"
        current = self.state / (hashlib.sha256(record["name"].encode()).hexdigest() + ".json")
        current.write_text(json.dumps(record))
        cached = self.hgs(*args)["task"]
        self.assertEqual(cached["target"]["session"], "codex/renamed")
        self.assertEqual(cached["target"]["session_id"], task["target"]["session_id"])
