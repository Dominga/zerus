"""Task search through the real CLI and product task writers, in private state."""
from test_tasks import Fixture


class TaskSearch(Fixture):
    def search(self, query, stage="todo", limit=2, after=None, session=None, ok=True):
        args = ["list", "--project", "orchard", "--stage", stage,
                "--limit", str(limit), "--query", query]
        if after:
            args.extend(["--after", after])
        if session:
            args.extend(["--session", session])
        return self.hgs(*args, ok=ok)

    def test_short_and_project_qualified_refs(self):
        wanted = self.add("Find this reference")
        self.add("Another task")
        for query in (wanted["id"], wanted["ref"], wanted["ref"].upper()):
            with self.subTest(query=query):
                page = self.search(query)
                self.assertEqual([t["uuid"] for t in page["tasks"]], [wanted["uuid"]])
                self.assertEqual(page["query"], query)
        self.assertEqual(self.search("elsewhere/" + wanted["id"])["tasks"], [])

    def test_title_and_full_text_case_insensitively(self):
        title = self.add("Ordinary body", "--title", "Проверка интерфейса")
        body = self.add("Ordinary title\n" + "x" * 3000 + " Needle in the body")
        self.add("Unrelated")
        for query, wanted in (("ПРОВЕРКА", title), ("needle in the body", body)):
            with self.subTest(query=query):
                page = self.search(query)
                self.assertEqual([t["uuid"] for t in page["tasks"]], [wanted["uuid"]])
                self.assertLessEqual(len(page["tasks"][0]["text"].encode()), 2048)

    def test_search_is_literal(self):
        wanted = self.add("Find 100%_literal's text")
        self.add("Find 100 percent different text")
        page = self.search("100%_literal's")
        self.assertEqual([t["uuid"] for t in page["tasks"]], [wanted["uuid"]])

    def test_pages_search_beyond_the_initial_window_and_keeps_stage_counts(self):
        for n in range(51):
            self.add(f"Other task {n}")
        wanted = [self.add(f"Needle {n}") for n in range(5)]
        self.add("Needle backlog", "--backlog")
        page = self.search("Needle")
        found = []
        while True:
            self.assertLessEqual(len(page["tasks"]), 2)
            self.assertEqual(page["page"]["total"], 5)
            self.assertEqual(page["counts"]["todo"], 5)
            self.assertEqual(page["counts"]["backlog"], 1)
            found.extend(t["uuid"] for t in page["tasks"])
            cursor = page["page"]["next"]
            if not cursor:
                break
            page = self.search("Needle", after=cursor)
        self.assertEqual(found, [t["uuid"] for t in wanted])
        self.assertEqual(len(self.search("Needle", stage="backlog")["tasks"]), 1)

    def test_empty_result_has_no_more_pages_and_zero_counts(self):
        self.add("An existing task")
        page = self.search("No such task")
        self.assertEqual(page["tasks"], [])
        self.assertEqual(page["page"]["total"], 0)
        self.assertIsNone(page["page"]["next"])
        self.assertTrue(all(n == 0 for n in page["counts"].values()))

    def test_search_intersects_the_session_filter(self):
        self.agent("codex/one")
        self.agent("codex/two")
        own = self.add("Needle own", "--target", "codex/one")
        anyone = self.add("Needle anyone")
        self.add("Needle other", "--target", "codex/two")
        self.add("Different own", "--target", "codex/one")
        page = self.search("Needle", session="run_id:" + self.runs["codex/one"])
        self.assertEqual([t["uuid"] for t in page["tasks"]], [own["uuid"], anyone["uuid"]])
        self.assertEqual(page["counts"]["todo"], 2)

    def test_a_search_cursor_cannot_continue_another_query(self):
        self.add("Needle one")
        self.add("Needle two")
        cursor = self.search("Needle", limit=1)["page"]["next"]
        self.assertIn("cursor does not match", self.search("Other", after=cursor, ok=False))
