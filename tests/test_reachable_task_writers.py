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
        (self.swarm.root / "a/config").write_text('HGS_SELF="a"\nHGS_PEERS="b c"\n')
        (self.swarm.root / "c/config").write_text('HGS_SELF="c"\nHGS_PEERS="a"\n')
        for node, peers in [("a", ["b", "c"]), ("b", ["a"]), ("c", ["a"])]:
            path = self.swarm.root / node / "machines.local.json"
            profile = next(iter(json.loads(path.read_text())["machines"].values()))
            path.write_text(json.dumps({"machines": {peer: profile for peer in peers}}))
        for node, peer in [("a", "b"), ("b", "a"), ("c", "a"), ("a", "c")]:
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
        for name in "abc":
            self.swarm.call(name, "hello")
        (self.swarm.root / "offline-b").touch()
        started = self.start(task["uuid"])
        # Ordinary direct-start acknowledges legacy history until an actual
        # execution floor exists. A schema-8 custody seal alone cannot mint
        # an attempt or convert this acknowledgement into fence authority.
        self.assertEqual(started["reporting"], "legacy")
        self.assertNotIn("report_attempt", started)
        self.assertEqual(started["task"]["execution"]["attempts"], 0)
        again = self.start(task["uuid"])
        self.assertEqual(again["reporting"], "legacy")
        self.assertEqual(again["task"]["execution"]["attempts"], 0)
        self.assertEqual(sum(entry["kind"] == "started" for entry in again["task"]["log"]), 1)
        self.assertFalse(list((self.swarm.root / "a/state/replica").glob(f"*/fence/{task['uuid']}.json")))
        self.swarm.call("a", "sync")
        self.assertTrue(self.swarm.task("c", "get", task["uuid"], "--project", "a")["task"])

    def test_ordinary_stage_and_comment_do_not_start_or_certify_a_task(self):
        task = self.swarm.task("a", "add", "--project", "a", text="Ordinary task, no Start")["task"]
        (self.swarm.root / "offline-b").touch()
        synced = subprocess.run([str(test_swarm.HGS), "swarm", "sync"], env=self.env,
                                text=True, capture_output=True, timeout=60)
        self.assertEqual(synced.returncode, 0, synced.stdout + synced.stderr)
        env = dict(self.env, HGS_SESSION=self.name, HGS_RUN_ID=self.run)
        for action, extra, text in [("stage", ["backlog"], None), ("comment", [], "An ordinary comment")]:
            result = subprocess.run([str(test_swarm.HGS), "task", action, task["uuid"], *extra,
                                     "--project", "a", "--json"], cwd=self.repo, env=env,
                                    input=text, text=True, capture_output=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        current = json.loads(result.stdout)["task"]
        self.assertEqual(current["stage"], "backlog")
        self.assertFalse(any(entry["kind"] in {"started", "reported", "attempt"} for entry in current["log"]))
        self.assertFalse(list((self.swarm.root / "a/state/replica").glob(f"*/fence/{task['uuid']}.json")))


if __name__ == "__main__":
    unittest.main()
