"""Prior-run job evidence through real resume in a private fake-agent session."""
import hashlib
import json
import os
import unittest
import uuid

if os.name == "posix":
    import test_pause as pause
else:
    pause = None


@unittest.skipUnless(os.name == "posix" and pause.TMUX, "requires private Unix tmux")
class ResumeProcessEvidence(pause.Harness if pause else unittest.TestCase):
    def test_resume_retains_unsettled_jobs_without_current_run_authority(self):
        name = self.start("claude")
        self.hgs("pause", name)
        path = self.root / "state" / (hashlib.sha256(name.encode()).hexdigest() + ".json")
        previous = json.loads(path.read_text())
        command = "private fixture command"
        template = dict(owner="main", provider="claude", source="hooks", background=True,
                        conversation_id=previous["conversation_id"], command=command,
                        command_digest=hashlib.sha256(command.encode()).hexdigest(), command_truncated=False,
                        output="prior unresolved output marker", observed=False,
                        capabilities=dict(output=True, stop=False, stdin=False))
        retained = {}
        for i, status in enumerate(("unknown", "running", "approval")):
            job = dict(template, id=f"prior-{i}", call_id=f"prior-call-{i}", status=status,
                       run_id=previous["run_id"] if i != 1 else str(uuid.uuid4()),
                       native_id=f"prior-native-{i}", observed=i != 0)
            retained[job["id"]] = job
        previous["shell_jobs"] = dict(retained)
        for i in range(40):
            job = dict(template, id=f"settled-{i}", call_id=f"settled-call-{i}", status="completed",
                       run_id=previous["run_id"], updated_at=i, ended_at=i, exit_code=0,
                       output="x" * 4096, output_truncated=False)
            previous["shell_jobs"][job["id"]] = job
        path.write_text(json.dumps(previous))
        old_run = previous["run_id"]
        self.resume(name)
        after = self.binding(name)
        self.assertNotEqual(after["run_id"], old_run)
        self.assertEqual(after["conversation_id"], previous["conversation_id"])
        self.assertFalse(list((self.root / "state/archives").glob("*.json")))
        for key, job in retained.items():
            self.assertEqual(after.get("shell_jobs", {}).get(key), job,
                             "resume must retain exact prior-run unresolved evidence")
        jobs = after["shell_jobs"]
        self.assertLessEqual(len(jobs), len(retained) + 16, "settled history remains bounded")
        for job in jobs.values():
            if job["status"] == "completed":
                self.assertLessEqual(len(job["output"].encode()), 1024)
                self.assertTrue(job["output_truncated"])
        self.assertLess(len(json.dumps(after).encode()), 16 << 20)
        projected = json.loads(self.hgs("processes", name, "--json"))
        self.assertEqual(projected["active_count"], 0, "retained evidence is not current work")
        items = {job["id"]: job for job in projected["items"]}
        for key in retained:
            self.assertEqual(items[key]["status"], "unknown")
            self.assertTrue(items[key]["stale"])
        # Neither the previous run nor the resumed run grants control of old jobs.
        for run in (old_run, after["run_id"]):
            self.hgs("processes", name, "--stop", "prior-0", "--run", run,
                     "--conversation", after["conversation_id"], rc=1)
        self.assertEqual(self.binding(name)["shell_jobs"], jobs)


if __name__ == "__main__":
    unittest.main()
