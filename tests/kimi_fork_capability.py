"""Current native Kimi Fork qualification in the caller's disposable home."""
import json
import subprocess

import test_pause as lifecycle


def check(case):
    name = case.launch()
    text = "Private native Fork source context"
    receipt = case.send(name, case.payload(name, text))
    case.until(lambda: case.settled(name, receipt), name)
    source = case.binding(name)
    case.assert_process(name, "fork-source-ready")

    # Independent utility control: Ready does not release Kimi's instance lock.
    def histories():
        return {path.relative_to(case.home) for path in (case.home / "sessions").glob("*/*") if path.is_dir()}
    before = histories()
    case.assertTrue(any(path.name == source["conversation_id"] for path in before))
    native = subprocess.run([str(case.image), "fork", source["conversation_id"], "--yes"],
                            env=case.env, cwd=case.project, capture_output=True, text=True, timeout=40)
    case.assertNotEqual(native.returncode, 0, native.stdout + native.stderr)
    case.assertIn("This session is in use by another Kimi Code CLI instance", native.stderr)
    case.assertNotRegex(native.stdout, r"(?m)^Forked to ")
    case.assertEqual(histories(), before, "a refused native Fork must not create child history")
    live = case.inspect(name)
    case.assertFalse(live["fork_supported"], live)
    case.assertEqual(live["fork_reason"], "Pause or close this Kimi session before forking its saved context")
    case.assertNotIn("fork", live["allowed_actions"])
    refused = subprocess.run([str(lifecycle.HGS), "fork", name, "-n", "refused", "-d"],
                             env=case.env, capture_output=True, text=True, timeout=40)
    case.assertEqual(refused.returncode, 1, refused.stdout + refused.stderr)
    case.assertIn(live["fork_reason"], refused.stderr)
    case.assertNotEqual(subprocess.run([str(case.bin / "tmux"), "has-session", "-t", "=kimi/p/refused"],
                                      env=case.env, capture_output=True).returncode, 0)
    for key in ("run_id", "conversation_id", "pid", "process_start"):
        case.assertEqual(case.binding(name)[key], source[key])
    case.assert_process(name, "after-refused-live-fork")

    # Only this explicit fixture action closes the instance; Fork never pauses it.
    case.hgs("pause", name)
    case.until(lambda: subprocess.run([str(case.bin / "tmux"), "has-session", "-t", "=" + name],
                                     env=case.env, capture_output=True).returncode != 0, name)
    paused = case.inspect(name)
    case.assertTrue(paused["fork_supported"], paused)
    case.hgs("fork", name, "-n", "paused-branch", "-d",
             "--expected-run-id", source["run_id"], "--expected-conversation-id", source["conversation_id"])
    child = "kimi/p/paused-branch"
    case.until(lambda: case.binding(child).get("conversation_id") and
               "pause" in case.inspect(child).get("allowed_actions", []), child)
    forked = case.binding(child)
    case.assertNotEqual(forked["conversation_id"], source["conversation_id"])
    case.assertEqual(forked["fork_parent_id"], source["conversation_id"])
    case.assert_process(child, "native-paused-fork")
    reply = case.send(child, case.payload(child, "Continue the private fork"))
    case.until(lambda: case.settled(child, reply), child)
    case.assertIn(text, json.dumps(case.requests[-1]["messages"]))
    case.hgs("pause", child)

    case.hgs("archive", name)
    archives = [json.loads(path.read_text()) for path in (case.root / "state/archives").glob("*.json")]
    archived = next(row for row in archives if row["conversation_id"] == source["conversation_id"])
    state = json.loads(case.hgs("inspect", name, "--archive", archived["archive_id"]))
    case.assertTrue(state["fork_supported"], state)
    case.hgs("fork", name, "-n", "archive-branch", "-d", "--archive", archived["archive_id"])
    child = "kimi/p/archive-branch"
    case.until(lambda: case.binding(child).get("conversation_id") and
               "pause" in case.inspect(child).get("allowed_actions", []), child)
    ended = case.binding(child)
    case.assertNotEqual(ended["conversation_id"], source["conversation_id"])
    case.assertNotEqual(ended["conversation_id"], forked["conversation_id"])
    case.assertEqual(ended["fork_parent_id"], source["conversation_id"])
    case.assert_process(child, "native-archived-fork")
    case.assertNotEqual(subprocess.run([str(case.bin / "tmux"), "has-session", "-t", "=" + name],
                                      env=case.env, capture_output=True).returncode, 0)
    print("KIMI_FORK_CAPABILITY=" + json.dumps(dict(version=case.version, sha256=case.image_digest,
          live_native_exit=native.returncode, live_offered=False, paused_offered=True,
          archived_offered=True, parent=source["conversation_id"],
          paused_child=forked["conversation_id"], archived_child=ended["conversation_id"])), flush=True)
