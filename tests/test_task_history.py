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

    def test_metadata_pages_name_files_and_task_attachment_returns_them(self):
        import base64
        spec = dict(name="spec.txt", mime="text/plain", reference="[File #1]",
                    data_base64=base64.b64encode(b"the spec").decode())
        task = self.add(json.dumps(dict(text="Read [File #1]", attachments=[spec])), "--content-json")
        log = dict(name="log.txt", mime="text/plain", reference="[File #1]",
                   data_base64=base64.b64encode(b"a log").decode())
        self.hgs("comment", task["uuid"], "--project", "orchard", "--content-json",
                 text=json.dumps(dict(text="See [File #1]", attachments=[log])))
        args = ("get", task["uuid"], "--project", "orchard", "--history-limit", "80")
        full = self.hgs(*args)["task"]
        page = self.hgs(*args, "--attachments", "metadata")["task"]
        self.assertNotIn("data_base64", json.dumps(page))
        self.assertEqual(page["attachments"], [dict(name="spec.txt", mime="text/plain", reference="[File #1]",
                                                    size=8, sha256=hashlib.sha256(b"the spec").hexdigest())])
        self.assertEqual(full["attachments"][0]["data_base64"], spec["data_base64"])
        self.assertEqual([e["kind"] for e in page["log"]], [e["kind"] for e in full["log"]])
        noted = next(e for e in page["log"] if e.get("text") == "See [File #1]")
        self.assertEqual(noted["attachments"][0]["sha256"], hashlib.sha256(b"a log").hexdigest())
        self.assertTrue(all("id" in e for e in page["log"]))
        self.assertTrue(all("id" not in e for e in full["log"]))
        # One file at a time, from the task or one of its entries.
        own = self.hgs("attachment", task["uuid"], "--project", "orchard", "--file", "1")
        self.assertEqual((own["name"], own["data_base64"], own["size"], own["entry"]),
                         ("spec.txt", spec["data_base64"], 8, None))
        self.assertEqual(own["sha256"], page["attachments"][0]["sha256"])
        entry = self.hgs("attachment", task["uuid"], "--project", "orchard", "--file", "1", "--entry", noted["id"])
        self.assertEqual((entry["name"], base64.b64decode(entry["data_base64"])), ("log.txt", b"a log"))
        for refused, message in [
            (("--file", "2"), "there is no file 2"),
            (("--file", "0"), "--file is a number from 1"),
            (("--file", "1", "--entry", "elsewhere:1@1"), "cursor does not match this task"),
        ]:
            self.assertIn(message, self.hgs("attachment", task["uuid"], "--project", "orchard", *refused, ok=False))
        self.assertIn("--attachments requires --history-limit",
                      self.hgs("get", task["uuid"], "--project", "orchard", "--attachments", "metadata", ok=False))
        self.assertIn("--attachments takes metadata", self.hgs(*args, "--attachments", "files", ok=False))

