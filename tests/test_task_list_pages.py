"""Paged task lists from the disposable task index, beside the unchanged
legacy `task list` (zerus-cg5h.2)."""
import hashlib
import os
from test_tasks import Fixture


class TaskListPages(Fixture):
    def page(self, stage, limit, *args):
        return self.hgs("list", "--project", "orchard", "--stage", stage, "--limit", str(limit), *args)

    def walk(self, stage, limit):
        """Every row of a stage, forwards by `next`, then backwards by `previous`."""
        forwards, cursor = [], None
        while True:
            page = self.page(stage, limit, *(("--after", cursor) if cursor else ()))
            self.assertEqual(page["page"]["schema"], 1)
            self.assertLessEqual(len(page["tasks"]), limit)
            self.assertEqual(page["page"]["position"], len(forwards))
            forwards += page["tasks"]
            cursor = page["page"]["next"]
            if cursor is None:
                break
        # Back from the last page: every earlier row, each page in order.
        last, backwards, cursor = page["tasks"], [], page["page"]["previous"]
        while cursor is not None:
            page = self.page(stage, limit, "--before", cursor)
            self.assertEqual(page["page"]["position"], len(forwards) - len(last) - len(backwards) - len(page["tasks"]))
            backwards = page["tasks"] + backwards
            cursor = page["page"]["previous"]
        self.assertEqual(backwards + last, forwards)
        return forwards

    def test_pages_hold_the_legacy_rows_in_order_and_poll_by_revision(self):
        for n in range(7):
            self.add(f"Task {n}", *(("--backlog",) if n % 3 == 0 else ()))
        legacy = self.hgs("list", "--project", "orchard")
        for stage in ["todo", "backlog", "active", "review", "done"]:
            expected = [t for t in legacy["tasks"] if t["stage"] == stage]
            for limit in (1, 2, 200):
                self.assertEqual(self.walk(stage, limit), expected, (stage, limit))
        first = self.page("todo", 2)
        self.assertEqual(first["counts"], {"review": 0, "active": 0, "todo": 4, "backlog": 3, "done": 0})
        self.assertEqual(first["page"]["total"], 4)
        self.assertEqual((first["damaged"], first["damaged_total"]), ([], 0))
        self.assertEqual(first["project"]["name"], "orchard")
        # Nothing changed: the poll answers without rows.
        unchanged = self.page("todo", 2, "--if-revision", first["revision"])
        self.assertEqual(unchanged, {"project": first["project"], "unchanged": True,
                                     "revision": first["revision"]})
        # A journal-only note changes the revision and the row's history revision.
        uuid = first["tasks"][0]["uuid"]
        self.hgs("comment", uuid, "--project", "orchard", text="A note")
        after = self.page("todo", 2, "--if-revision", first["revision"])
        self.assertNotEqual(after["revision"], first["revision"])
        self.assertNotEqual(after["tasks"][0]["history_revision"], first["tasks"][0]["history_revision"])
        # The legacy list is unchanged by any of this.
        self.assertNotIn("page", self.hgs("list", "--project", "orchard"))

    def test_a_cursor_holds_its_place_as_tasks_arrive_and_names_its_list(self):
        for n in range(5):
            self.add(f"Task {n}")
        first = self.page("todo", 2)
        second = self.page("todo", 2, "--after", first["page"]["next"])
        self.add("At the top", "--position", "1")
        again = self.page("todo", 2, "--after", first["page"]["next"])
        self.assertEqual(again["tasks"], second["tasks"])
        self.assertEqual(again["page"]["position"], second["page"]["position"] + 1)
        refused = self.hgs("list", "--project", "orchard", "--stage", "backlog", "--limit", "2",
                           "--after", first["page"]["next"], ok=False)
        self.assertIn("cursor does not match this list", refused)

    def test_long_text_is_cut_in_pages_only(self):
        task = self.add("ж" * 3000)
        row = self.page("todo", 1)["tasks"][0]
        self.assertLessEqual(len(row["text"].encode()), 2048)
        self.assertTrue(row["text_truncated"])
        self.assertTrue(("ж" * 3000).startswith(row["text"]))
        self.assertEqual(self.hgs("list", "--project", "orchard")["tasks"][0]["text"], "ж" * 3000)
        self.assertEqual(self.hgs("get", task["uuid"], "--project", "orchard")["task"]["text"], "ж" * 3000)

    def test_bad_requests_are_refused(self):
        self.add("Task")
        for args, message in [
            (("--stage", "todo", "--limit", "0"), "--limit must be between 1 and 200"),
            (("--stage", "todo", "--limit", "201"), "--limit must be between 1 and 200"),
            (("--stage", "todo", "--limit", "x"), "--limit must be between 1 and 200"),
            (("--limit", "5"), "--limit needs --stage"),
            (("--after", "c"), "--after needs --stage and --limit"),
            (("--stage", "todo", "--if-revision", "r"), "--if-revision needs --stage and --limit"),
            (("--stage", "todo", "--limit", "5", "--after", "a", "--before", "b"), "use --after or --before, not both"),
            (("--stage", "todo", "--limit", "5", "--before", "garbage"), "cursor does not match this list"),
        ]:
            refused = self.hgs("list", "--project", "orchard", *args, ok=False)
            self.assertIn(message, refused, args)

    def test_a_damaged_index_is_rebuilt_and_never_fails_a_read(self):
        for n in range(3):
            self.add(f"Task {n}")
        good = self.page("todo", 10)
        project = good["project"]["id"]
        folder = self.state / "replica" / hashlib.sha256(project.encode()).hexdigest()
        index = folder / "task-index-v1.sqlite3"
        self.assertTrue(index.exists())
        self.assertEqual(index.stat().st_mode & 0o777, 0o600)
        index.write_bytes(b"not a database")
        for suffix in ("-wal", "-shm"):
            if os.path.exists(f"{index}{suffix}"):
                os.remove(f"{index}{suffix}")
        rebuilt = self.page("todo", 10)
        self.assertEqual(rebuilt["tasks"], good["tasks"])
        self.assertNotEqual(rebuilt["revision"], good["revision"])
        # Short IDs still find their task, and an unknown one is refused as before.
        short = good["tasks"][0]["id"]
        self.assertEqual(self.hgs("get", f"orchard/{short}")["task"]["uuid"], good["tasks"][0]["uuid"])
        refused = self.hgs("get", "orchard/T-zzzzzz", ok=False)
        self.assertIn("no task T-zzzzzz in this project", refused)


if __name__ == "__main__":
    import unittest
    unittest.main()
