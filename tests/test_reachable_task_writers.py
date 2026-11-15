#!/usr/bin/env python3
"""Reachable task cutover through real CLI processes, replica files and Git.

Only SSH is a local subprocess transport; every command uses private state.
The direct-start caller is this fixture's pinned process, and offers no input.
"""
import copy
import json
import os
import subprocess
import unittest
import uuid

from tests import test_swarm, test_tasks


class ReachableTaskWriters(unittest.TestCase):
    def setUp(self):
        self.swarm = test_swarm.Swarm(methodName="runTest")
        self.swarm.setUp()
        self.addCleanup(self.swarm.doCleanups)
        self.repo = self.swarm.root / "project-repo"
        self.repo.mkdir()
        subprocess.run(["git", "init", "--quiet", "--initial-branch=main", str(self.repo)],
                       env=dict(self.swarm.env, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull),
                       check=True, capture_output=True, timeout=20)
        self.assertEqual(subprocess.check_output(["git", "-C", str(self.repo), "rev-parse", "--show-toplevel"],
                                                env=dict(self.swarm.env, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull),
                                                text=True).strip(), str(self.repo))
        # Use direct verified a/b and a/c routes. The absent b must not hide c.
        (self.swarm.root / "c/config").write_text('HGS_SELF="c"\nHGS_PEERS="a"\n')
        for node, peer in [("a", "b"), ("b", "a"), ("a", "c"), ("c", "a")]:
            self.swarm.join(node, peer)
        self.swarm.converge()
        before = self.swarm.call("a", "get")
        desired = copy.deepcopy(before["organization"])
        project = next(project for project in desired["projects"] if project["id"] == "a")
        project["folders"][0]["path"] = str(self.repo)
        self.swarm.patch("a", before, desired)
        self.name, self.run = "codex/reachable-fixture", "run-" + str(uuid.uuid4())
        self.env = dict(self.swarm.env, HGS_CONFIG_DIR=str(self.swarm.root / "a"),
                        HGS_STATE_DIR=str(self.swarm.root / "a/state"))
        test_tasks.agent_record(self.env["HGS_STATE_DIR"], self.env, self.name, self.run)
        test_tasks.join(self.env, "a", self.name)

    def start(self, reference):
        result = subprocess.run([str(test_swarm.HGS), "task", "start", reference, "--project", "a", "--json"],
                                cwd=self.repo, env=dict(self.env, HGS_SESSION=self.name, HGS_RUN_ID=self.run),
                                text=True, capture_output=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return json.loads(result.stdout)

    def test_an_absent_writer_does_not_block_an_available_git_projects_direct_start(self):
        task = self.swarm.task("a", "add", "--project", "a", "--target", self.name,
                               text="One original on the reachable computer")["task"]
        ids = {name: self.swarm.call(name, "hello")["node_id"] for name in "abc"}
        (self.swarm.root / "offline-b").touch()
        started = self.start(task["ref"])
        origin = started["report_attempt"]
        self.assertEqual(started["reporting"], "attempt")
        self.assertEqual(started["task"]["execution"]["attempts"], 1)
        again = self.start(task["ref"])
        self.assertEqual(again["report_attempt"], origin, "acknowledgement looks up the original")
        self.assertEqual(again["task"]["execution"]["attempts"], 1)
        certificate = next((self.swarm.root / "a/state/replica").glob(f"*/fence/{task['uuid']}.json"))
        proof = json.loads(certificate.read_text())
        self.assertEqual(proof["format"], 2)
        self.assertIn(ids["b"], proof["not_cut_over"])
        self.assertNotIn(ids["b"], proof["nodes"])
        self.assertIn(ids["c"], proof["nodes"])
        self.assertTrue(self.swarm.task("c", "get", task["ref"], "--project", "a")["task"])

    def test_ordinary_stage_and_comment_do_not_start_or_certify_a_task(self):
        task = self.swarm.task("a", "add", "--project", "a", text="Ordinary task, no Start")["task"]
        (self.swarm.root / "offline-b").touch()
        synced = subprocess.run([str(test_swarm.HGS), "swarm", "sync"], env=self.env,
                                text=True, capture_output=True, timeout=60)
        self.assertEqual(synced.returncode, 0, synced.stdout + synced.stderr)
        env = dict(self.env, HGS_SESSION=self.name, HGS_RUN_ID=self.run)
        for action, extra, text in [("stage", ["backlog"], None), ("comment", [], "An ordinary comment")]:
            result = subprocess.run([str(test_swarm.HGS), "task", action, task["ref"], *extra,
                                     "--project", "a", "--json"], cwd=self.repo, env=env,
                                    input=text, text=True, capture_output=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        current = json.loads(result.stdout)["task"]
        self.assertEqual(current["stage"], "backlog")
        self.assertFalse(any(entry["kind"] in {"started", "reported", "attempt"} for entry in current["log"]))
        self.assertFalse(list((self.swarm.root / "a/state/replica").glob(f"*/fence/{task['uuid']}.json")))


if __name__ == "__main__":
    unittest.main()
