"""Keep cache warm: setting, worker round and silent ping turns, with private fake agents only."""
import datetime
import fcntl
import os
import json
from pathlib import Path
import subprocess
import sys
import time
import unittest
import uuid

import test_input as fixtures
import test_pause

TEXT = "[Zerus keep-warm] Reply with exactly OK and do nothing else."
NOTICE = ("<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n"
          "<summary>Background command \"watch\" completed</summary>\n</task-notification>")
PEER = ("<zerus-message from=\"peer/session\" host=\"peerhost\" "
        "reference=\"zerus://session?machine=11111111-1111-4111-8111-111111111111"
        "&amp;host=peerhost&amp;name=peer%2Fsession"
        "&amp;conversation=22222222-2222-4222-8222-222222222222"
        "&amp;run=33333333-3333-4333-8333-333333333333\">\n"
        "hello from a peer\n</zerus-message>")


class KeepWarm(unittest.TestCase):
    tearDown = fixtures.InputTransport.tearDown

    def setUp(self):
        fixtures.InputTransport.setUp(self)
        # Hold the recovery executor lock: enabling keep-warm then starts no
        # detached worker, and `recovery tick` runs each round in the test.
        directory = self.state / "recovery"
        directory.mkdir(exist_ok=True)
        self.worker_lock = (directory / "worker.lock").open("a")
        fcntl.flock(self.worker_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.addCleanup(self.worker_lock.close)

    script = fixtures.InputTransport.script
    tmux = fixtures.InputTransport.tmux
    wait_for = fixtures.InputTransport.wait_for
    write_record = fixtures.InputTransport.write_record
    screen = fixtures.InputTransport.screen
    received = fixtures.InputTransport.received

    def command(self, *args, payload=None, ok=True):
        result = subprocess.run([str(fixtures.HGS), *args], env=self.env, text=True, capture_output=True,
                                input=json.dumps(payload) if payload is not None else None, timeout=15)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        return json.loads(result.stdout) if result.stdout.strip() else result.stderr

    def change(self, revision=0, ok=True, enabled=True, **duration):
        body = dict(expected_run_id=self.run_id, expected_conversation_id=self.conversation_id,
                    expected_revision=revision, enabled=enabled, **duration)
        return self.command("keep-warm", self.name, "--json", payload=body, ok=ok)

    def inspect(self):
        return self.command("inspect", self.name)

    def listed(self):
        rows = self.command("ls", "--local", "--json")["sessions"]
        return next(row for row in rows if row["name"] == self.name)

    def test_setting_is_revisioned_listed_and_turned_off(self):
        before = time.time()
        changed = self.change(hours=3)
        self.assertTrue(changed["ok"] and changed["applied"], changed)
        warm = changed["keep_warm"]
        self.assertTrue(warm["enabled"])
        self.assertGreaterEqual(warm["until"], before + 3 * 3600)
        self.assertLessEqual(warm["until"], time.time() + 3 * 3600)
        listed = self.listed()["keep_warm"]
        self.assertEqual((listed["enabled"], listed["revision"], listed["turn_active"]), (True, 1, False))
        self.assertEqual(self.inspect()["keep_warm"]["revision"], 1)
        # A second desktop with the old revision is refused, not merged.
        conflict = self.change(revision=0, hours=5, ok=False)
        self.assertEqual(conflict["error"], "conflict")
        always = self.change(revision=1, always=True)
        self.assertIsNone(always["keep_warm"]["until"])
        off = self.change(revision=2, enabled=False)
        self.assertFalse(off["keep_warm"]["enabled"])
        self.assertEqual(self.listed()["keep_warm"]["state"], "off")

    def test_details_explain_the_lifetime_or_why_the_agent_is_not_eligible(self):
        self.record.update(model="gpt-6.1-sol", recovery_success_at=time.time() - 60)
        self.write_record()
        details = self.inspect()["keep_warm"]
        self.assertEqual((details["supported"], details["ttl_seconds"], details["interval_seconds"],
                          details["ttl_source"]), (True, 1800, 1440, "provider_default"))
        self.record["model"] = "gpt-5.5"
        self.write_record()
        details = self.inspect()["keep_warm"]
        self.assertEqual((details["supported"], details["reason_code"]), (False, "codex_model"))
        refused = self.change(hours=1, ok=False)
        self.assertEqual(refused["error"], "unsupported")
        self.assertIn("GPT-5.6", refused["reason"])
        self.assertFalse((self.state / "keep-warm.json").exists())

    def hook(self, kind, **values):
        import hook_caller_fixtures
        env=dict(self.env,HGS_SESSION=self.name,HGS_RUN_ID=self.run_id)
        event=dict(hook_event_name=kind,session_id=self.conversation_id,**values)
        result=hook_caller_fixtures.run(self,event,env) if self.record['agent']=='claude' else subprocess.run(
            [str(fixtures.HGS),'__state','hook'],env=env,input=json.dumps(event),text=True,capture_output=True,timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)

    def pending_ping(self):
        record = json.loads(self.record_path.read_text())
        record["keep_warm_pending"] = dict(request_id="request-1", ping="ping-1", submitted_at=time.time())
        self.record_path.write_text(json.dumps(record))

    def test_a_ping_turn_creates_no_reply_and_keeps_the_visible_session(self):
        for agent in ("claude", "codex", "kimi"):
            with self.subTest(agent=agent):
                self.record.update(agent=agent, conversation_id=self.conversation_id)
                self.write_record()
                self.exercise_ping_turn()
                # The next agent starts from a fresh conversation.
                self.conversation_id = str(__import__("uuid").uuid4())
                self.record = json.loads(self.record_path.read_text())
                for key in ("keep_warm_turn", "keep_warm_quiet", "keep_warm_pending"):
                    self.record.pop(key, None)

    def exercise_ping_turn(self):
        self.hook("UserPromptSubmit", prompt="Fix the tests")
        self.hook("Stop", last_assistant_message="Done.")
        before = self.inspect()
        self.assertTrue(before["reply_id"])
        self.pending_ping()
        self.hook("UserPromptSubmit", prompt=TEXT)
        self.assertTrue(self.listed()["keep_warm"]["turn_active"])
        self.hook("Stop", last_assistant_message="OK")
        after = self.inspect()
        self.assertEqual(after["reply_id"], before["reply_id"])
        self.assertEqual((after["last_message"], after["prompt"]), ("Done.", "Fix the tests"))
        self.assertEqual(after["last_event_at"], before["last_event_at"])
        row = self.listed()
        self.assertEqual((row["reply_id"], row["prompt"], row["last_event_at"], row["keep_warm"]["turn_active"]),
                         (before["reply_id"], "Fix the tests", before["last_event_at"], False))
        ping = [event for event in after["events"] if event.get("turn_origin") == "keep_warm"]
        self.assertEqual([(event["type"], event.get("origin")) for event in ping],
                         [("UserPromptSubmit", "keep_warm"), ("Stop", None)])
        # The person's next message is an ordinary turn with its own reply.
        self.hook("UserPromptSubmit", prompt="Next task")
        self.hook("Stop", last_assistant_message="Done again.")
        final = self.inspect()
        self.assertNotEqual(final["reply_id"], before["reply_id"])
        self.assertEqual(final["last_message"], "Done again.")

    def begin_ping(self, agent="claude"):
        """The person's finished turn, then one ping typed by the guarded path."""
        self.record.update(agent=agent)
        self.write_record()
        self.hook("UserPromptSubmit", prompt="Fix the tests")
        self.hook("Stop", last_assistant_message="Done.")
        before = self.inspect()
        self.pending_ping()
        self.hook("UserPromptSubmit", prompt=TEXT)
        return before

    def marked(self, after):
        return [(event["type"], event.get("origin"), event.get("turn_origin"))
                for event in after["events"]
                if event.get("origin") == "keep_warm" or event.get("turn_origin") == "keep_warm"]

    def test_a_background_completion_after_a_ping_keeps_the_person_prompt_and_reply(self):
        before = self.begin_ping()
        self.hook("Stop", last_assistant_message="OK")
        self.hook("UserPromptSubmit", prompt=NOTICE)
        self.hook("Stop", last_assistant_message="The watcher finished; all jobs are green.")
        after = self.inspect()
        self.assertNotEqual(after["reply_id"], before["reply_id"])
        self.assertEqual((after["prompt"], after["last_message"]),
                         ("Fix the tests", "The watcher finished; all jobs are green."))
        self.assertFalse(self.listed()["keep_warm"]["turn_active"])
        # Only the ping's own prompt and Stop are marked; the completion is visible.
        self.assertEqual(self.marked(after),
                         [("UserPromptSubmit", "keep_warm", "keep_warm"), ("Stop", None, "keep_warm")])
        by_detail = {event.get("detail"): event for event in after["events"]}
        completion = by_detail['Background command "watch" completed']
        self.assertEqual((completion["origin"], completion.get("turn_origin")), ("task_notification", None))
        self.assertIsNone(by_detail["The watcher finished; all jobs are green."].get("turn_origin"))

    def test_a_background_completion_queued_during_a_ping_keeps_the_person_prompt_and_reply(self):
        before = self.begin_ping()
        self.hook("UserPromptSubmit", prompt=NOTICE)  # queued while the ping runs
        self.hook("Stop", last_assistant_message="OK")  # the joined Stop ends real work
        self.hook("Stop", last_assistant_message="The watcher finished; all jobs are green.")
        after = self.inspect()
        self.assertNotEqual(after["reply_id"], before["reply_id"])
        self.assertEqual((after["prompt"], after["last_message"]),
                         ("Fix the tests", "The watcher finished; all jobs are green."))
        # The joined turn holds more than the ping, so only the ping prompt is marked.
        self.assertEqual(self.marked(after), [("UserPromptSubmit", "keep_warm", "keep_warm")])
        by_detail = {event.get("detail"): event for event in after["events"]}
        self.assertIsNone(by_detail["OK"].get("turn_origin"), "a joined Stop stays visible")
        self.assertIsNone(by_detail["The watcher finished; all jobs are green."].get("turn_origin"))

    def test_a_persons_message_during_a_ping_keeps_the_persons_prompt(self):
        before = self.begin_ping()
        self.hook("UserPromptSubmit", prompt="Also check CI")
        self.hook("Stop", last_assistant_message="CI is green.")
        after = self.inspect()
        self.assertNotEqual(after["reply_id"], before["reply_id"])
        self.assertEqual((after["prompt"], after["last_message"]), ("Also check CI", "CI is green."))
        self.assertEqual(self.marked(after), [("UserPromptSubmit", "keep_warm", "keep_warm")])
        self.assertFalse(self.listed()["keep_warm"]["turn_active"])

    def test_a_peer_message_during_a_ping_keeps_the_person_prompt(self):
        before = self.begin_ping()
        self.hook("UserPromptSubmit", prompt=PEER)
        self.hook("Stop", last_assistant_message="Handled the peer message.")
        after = self.inspect()
        self.assertNotEqual(after["reply_id"], before["reply_id"])
        self.assertEqual((after["prompt"], after["last_message"]),
                         ("Fix the tests", "Handled the peer message."))
        self.assertEqual(self.marked(after), [("UserPromptSubmit", "keep_warm", "keep_warm")])
        peer = next(event for event in after["events"] if event.get("origin") == "peer_message")
        self.assertEqual(peer["detail"], "Message from peer/session")
        self.assertIsNone(peer.get("turn_origin"))

    def test_a_prompt_that_only_starts_with_the_ping_text_stays_visible(self):
        self.begin_ping()
        text = TEXT + " Then review this task."
        self.hook("UserPromptSubmit", prompt=text)
        after = self.inspect()
        self.assertEqual(after["prompt"], text)
        self.assertFalse(self.listed()["keep_warm"]["turn_active"])
        self.hook("Stop", last_assistant_message="Real reply")
        self.assertEqual(self.inspect()["last_message"], "Real reply")

    def test_an_unmasking_prompt_restores_the_last_message_at_once(self):
        self.begin_ping()
        self.hook("Stop", last_assistant_message="OK")
        self.hook("UserPromptSubmit", prompt=NOTICE)
        after = self.inspect()
        self.assertEqual((after["prompt"], after["last_message"]), ("Fix the tests", "Done."))
        self.assertFalse(self.listed()["keep_warm"]["turn_active"])
        self.hook("Stop", last_assistant_message="Watcher finished.")
        self.assertEqual(self.inspect()["last_message"], "Watcher finished.")

    def test_a_failed_ping_starts_no_recovery(self):
        self.record["agent"] = "claude"
        self.write_record()
        directory = self.state / "recovery"
        (directory / "policy.json").write_text(json.dumps(dict(version=1, revision=1, enabled=True, enabled_at=1,
            service=True, rate_limit=True, network=True, delays=[15, 30, 60, 300])))
        self.screen("\x1b[2J\x1b[H❯ ", "2:0")
        self.pending_ping()
        self.hook("UserPromptSubmit", prompt=TEXT)
        self.hook("StopFailure", error="server_error", error_details="503 Service unavailable")
        self.command("recovery", "tick")
        self.assertIsNone(self.inspect().get("recovery"))
        self.assertEqual(self.received(), b"")

    def due(self, ago=1500):
        """An idle Codex session on a 30-minute cache whose last turn ended `ago` seconds back."""
        self.record.update(model="gpt-6.1-sol", last_event_at=time.time() - ago,
                           cache_touched=dict(at=time.time() - ago, conversation_id=self.conversation_id))
        self.write_record()

    def tick(self):
        self.command("recovery", "tick")

    def test_a_due_session_gets_exactly_one_ping_then_waits_for_its_turn(self):
        self.due()
        self.change(hours=1)
        self.tick()
        self.wait_for(lambda: self.received().endswith(b"\r"))
        raw = self.received()
        self.assertIn(TEXT.encode(), raw)
        self.assertEqual(raw.count(b"\r"), 1)
        warm = self.inspect()["keep_warm"]
        self.assertEqual(warm["state"], "warming")
        receipts = [json.loads(path.read_text()) for path in (self.state / "input_receipts").glob("*.json")]
        self.assertEqual([(r["origin"], r["status"], r["text"]) for r in receipts], [("keep_warm", "submitted", TEXT)])
        # Another round in the same window sends nothing more.
        self.tick()
        time.sleep(0.3)
        self.assertEqual(self.received(), raw)
        # The ping's turn ends; the next ping waits for the next window.
        self.hook("UserPromptSubmit", prompt=TEXT)
        self.hook("Stop", last_assistant_message="OK")
        self.tick()
        warm = self.inspect()["keep_warm"]
        self.assertEqual(warm["state"], "waiting")
        self.assertGreater(warm["next_at"], time.time() + 1400)
        self.assertEqual(self.received(), raw)

    def test_cold_busy_and_expired_sessions_get_no_ping(self):
        self.due(ago=1790)
        self.change(hours=1)
        self.tick()
        self.assertEqual(self.inspect()["keep_warm"]["state"], "cold")
        self.due()
        self.tick()
        self.assertEqual(self.inspect()["keep_warm"]["state"], "cold", "a missed window waits for a real turn")
        self.record.update(cache_touched=dict(at=time.time() - 1500, conversation_id=self.conversation_id),
                           keep_warm_real_stop_at=time.time(), activity="busy", phase="working")
        self.write_record()
        self.tick()
        self.assertEqual(self.inspect()["keep_warm"]["reason"], "The agent is working.")
        store = json.loads((self.state / "keep-warm.json").read_text())
        for entry in store["sessions"].values():
            entry["until"] = time.time() - 1
        (self.state / "keep-warm.json").write_text(json.dumps(store))
        self.record.update(activity="idle", phase="idle")
        self.write_record()
        self.tick()
        listed = self.listed()["keep_warm"]
        self.assertEqual((listed["enabled"], listed["state"]), (False, "off"))
        time.sleep(0.3)
        self.assertEqual(self.received(), b"")

    def wrap_tmux(self, case):
        """Replace the fixture's tmux with one that runs `case` (shell) first."""
        self.script("tmux", "#!/bin/sh\n" + case + "\nexec " + fixtures.TMUX + ' -S "' + str(self.socket) + '" -f /dev/null "$@"\n')

    def test_unknown_terminal_input_never_admits_a_ping(self):
        self.due()
        self.change(hours=1)
        self.wrap_tmux('[ "$1" = list-clients ] && exit 1')
        self.tick()
        time.sleep(0.3)
        self.assertEqual(self.received(), b"")
        self.assertEqual(self.inspect()["keep_warm"]["reason"], "Terminal input activity is unknown.")

    def test_a_slow_stage_that_crosses_the_cache_margin_types_nothing(self):
        self.due(ago=1765)  # 35 s of the 30-minute cache left
        self.change(hours=1)
        self.wrap_tmux('[ "$1" = load-buffer ] && sleep 7')
        self.tick()
        time.sleep(0.3)
        self.assertEqual(self.received(), b"")
        warm = self.inspect()["keep_warm"]
        # Refused by the final pre-input check, after the slow stage.
        self.assertEqual((warm["state"], warm["reason"]), ("waiting", "The cache may already have expired; nothing was submitted"))

    def test_a_crafted_owner_key_writes_nothing_outside_the_state(self):
        canary = self.root / "canary"
        (self.root / "canary.json").write_text("original")
        entry = dict(name=self.name, enabled=True, until=None, revision=1, set_at=time.time())
        (self.state / "keep-warm.json").write_text(json.dumps(dict(version=1, sessions={str(canary): entry})))
        self.tick()
        self.assertEqual((self.root / "canary.json").read_text(), "original")
        self.assertIsNone(self.listed()["keep_warm"])

    def test_a_special_store_file_never_blocks_a_listing(self):
        os.mkfifo(self.state / "keep-warm.json")
        started = time.monotonic()
        row = self.listed()
        self.assertLess(time.monotonic() - started, 10)
        self.assertIsNone(row["keep_warm"])
        self.tick()

    def test_a_malformed_worker_state_sends_nothing(self):
        self.due()
        self.change(hours=1)
        owner = next(iter(json.loads((self.state / "keep-warm.json").read_text())["sessions"]))
        state = self.state / "recovery" / "keep-warm"
        state.mkdir(parents=True, exist_ok=True)
        (state / f"{owner}.json").write_text('{"state":"dispatch')
        self.tick()
        time.sleep(0.3)
        self.assertEqual(self.received(), b"")
        self.assertEqual(self.inspect()["keep_warm"]["state"], "uncertain")
        self.tick()
        time.sleep(0.3)
        self.assertEqual(self.received(), b"", "an uncertain state waits for a real turn")

    def test_a_cleared_conversation_waits_for_its_own_turn(self):
        self.due()
        self.change(hours=1)
        # Clear: the same session continues in a new conversation.
        record = json.loads(self.record_path.read_text())
        record["conversation_id"] = self.conversation_id = "0f8b5f7e-3c2a-4d1e-9b6a-7c5d4e3f2a1b"
        self.record_path.write_text(json.dumps(record))
        self.tick()
        time.sleep(0.3)
        self.assertEqual(self.received(), b"")
        warm = self.inspect()["keep_warm"]
        self.assertEqual((warm["enabled"], warm["state"], warm["cache_expires_at"]), (True, "waiting", None))

    def test_a_legacy_label_rename_keeps_the_setting(self):
        self.change(hours=2)
        renamed = "codex/input-test/renamed"
        result = subprocess.run([str(fixtures.HGS), "rename", self.name, renamed], env=self.env, text=True,
                                capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.name = renamed
        warm = self.listed()["keep_warm"]
        self.assertEqual((warm["enabled"], warm["revision"]), (True, 1))
        store = json.loads((self.state / "keep-warm.json").read_text())
        self.assertEqual([entry["name"] for entry in store["sessions"].values()], [renamed])

    @unittest.skipUnless(sys.platform.startswith('linux'), 'uses the private child fd census to stage the lock wait')
    def test_an_id_move_while_a_setting_waits_cannot_recreate_its_old_key(self):
        from pathlib import Path
        self.record['session_id'] = str(uuid.uuid4())
        self.write_record()
        child = None
        try:
            with (self.state / 'keep-warm.lock').open('a+b') as setting:
                fcntl.flock(setting, fcntl.LOCK_EX | fcntl.LOCK_NB)
                body = dict(expected_run_id=self.run_id, expected_conversation_id=self.conversation_id,
                            expected_revision=0, enabled=True, hours=3)
                child = subprocess.Popen([str(fixtures.HGS), 'keep-warm', self.name, '--json'],
                                         env=self.env, text=True, stdin=subprocess.PIPE,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                child.stdin.write(json.dumps(body))
                child.stdin.close()
                child.stdin = None
                # This actual CLI has read its original binding before opening
                # this lock. It cannot advance while our original holds it.
                until = time.monotonic() + 5
                opened = False
                while child.poll() is None and time.monotonic() < until:
                    for fd in list(Path('/proc', str(child.pid), 'fd').iterdir())[:128]:
                        try:
                            opened |= os.readlink(fd) == str(self.state / 'keep-warm.lock')
                        except FileNotFoundError:
                            pass
                    if opened:
                        break
                    time.sleep(0.005)
                self.assertTrue(opened, 'the setting never opened its original lock')
                with (self.state / '.lock').open('a+b') as writer:
                    fcntl.flock(writer, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    # Dormant transition fixture only; production writers are
                    # still off. No native process, input or terminal changes.
                    test_pause.Harness.move_to_id_layout(self, self.name)
                fcntl.flock(setting, fcntl.LOCK_UN)
            stdout, stderr = child.communicate(timeout=10)
            self.assertNotEqual(child.returncode, 0, stdout + stderr)
            self.assertFalse((self.state / 'keep-warm.json').exists())
            self.assertEqual(self.received(), b'')
        finally:
            if child is not None and child.poll() is None:
                child.communicate(timeout=10)

    def runtime_file(self):
        owner = next(iter(json.loads((self.state / "keep-warm.json").read_text())["sessions"]))
        directory = self.state / "recovery" / "keep-warm"
        directory.mkdir(parents=True, exist_ok=True)
        return directory / f"{owner}.json"

    def test_an_unknown_worker_state_object_sends_nothing(self):
        self.due()
        self.change(hours=1)
        runtime = self.runtime_file()
        runtime.write_text(json.dumps(dict(state="uncertain", reason="kept", since=time.time())))
        self.tick()
        time.sleep(0.3)
        self.assertEqual(self.received(), b"")
        for unknown in ({}, {"state": "bogus"}, {"state": "dispatching"}):
            runtime.write_text(json.dumps(unknown))
            self.tick()
            time.sleep(0.3)
            self.assertEqual(self.received(), b"", unknown)
            self.assertEqual(self.inspect()["keep_warm"]["state"], "uncertain")

    def test_a_rename_interrupted_by_a_read_only_state_keeps_the_delivery_fence(self):
        self.interrupted_rename(0o500)

    def test_a_rename_that_cannot_see_the_state_keeps_the_delivery_fence(self):
        # Permission denied is not absence: the rename must not leave the fence behind.
        self.interrupted_rename(0o000)

    def interrupted_rename(self, mode):
        self.due()
        self.change(hours=1)
        old_runtime = self.runtime_file()
        old_runtime.write_text(json.dumps(dict(state="uncertain", reason="kept", since=time.time())))
        directory = old_runtime.parent
        directory.chmod(mode)
        try:
            renamed = "codex/input-test/renamed"
            failed = subprocess.run([str(fixtures.HGS), "rename", self.name, renamed], env=self.env, text=True,
                                    capture_output=True, timeout=15)
            self.assertNotEqual(failed.returncode, 0, "the rename must be interrupted")
            # A round may fail to record its state here; it must type nothing.
            self.command("recovery", "tick", ok=False)
            time.sleep(0.3)
            self.assertEqual(self.received(), b"", failed.stderr)
        finally:
            directory.chmod(0o700)
        # Any later command resumes the rename transaction.
        self.command("inspect", renamed, ok=False)
        self.name = renamed
        self.tick()
        self.tick()
        time.sleep(0.3)
        self.assertEqual(self.received(), b"")
        store = json.loads((self.state / "keep-warm.json").read_text())
        (owner, entry), = store["sessions"].items()
        self.assertEqual(entry["name"], renamed)
        self.assertEqual(json.loads((directory / f"{owner}.json").read_text())["state"], "uncertain")
        self.assertEqual(sorted(path.name for path in directory.glob("*.json")), [f"{owner}.json"])

    def test_an_unreadable_session_record_is_not_a_gone_session(self):
        self.due()
        self.change(hours=1)
        runtime = self.runtime_file()
        runtime.write_text(json.dumps(dict(state="uncertain", reason="kept", since=time.time())))
        self.record_path.chmod(0o000)
        try:
            self.command("recovery", "tick", ok=False)
        finally:
            self.record_path.chmod(0o600)
        store = json.loads((self.state / "keep-warm.json").read_text())
        self.assertEqual([entry["enabled"] for entry in store["sessions"].values()], [True])
        self.assertEqual(json.loads(runtime.read_text())["state"], "uncertain")
        self.tick()
        time.sleep(0.3)
        self.assertEqual(self.received(), b"")

    def transcript(self):
        self.record.update(agent="claude", model="claude-opus-5-5",
                           transcript=str(self.root / (self.conversation_id + ".jsonl")))
        self.write_record()
        return Path(self.record["transcript"])

    def assistant(self, at, identity):
        return json.dumps(dict(type="assistant", sessionId=self.conversation_id,
            timestamp=datetime.datetime.fromtimestamp(at, datetime.timezone.utc).isoformat(),
            message=dict(id=identity, model="claude-opus-5-5", usage=dict(input_tokens=10,
                cache_read_input_tokens=100, cache_creation_input_tokens=20, output_tokens=3,
                cache_creation=dict(ephemeral_1h_input_tokens=20))))) + "\n"

    def wait_for_usage_lock(self, pid):
        for _ in range(500):
            try:
                if "lock" in Path(f"/proc/{pid}/wchan").read_text():
                    return
            except OSError:
                pass
            time.sleep(0.01)
        self.fail("the round never waited for the usage cache lock")

    @unittest.skipUnless(sys.platform.startswith("linux"), "waits on the lock with /proc wchan")
    def test_an_append_while_a_round_waits_for_the_usage_lock_does_not_turn_cold(self):
        path = self.transcript()
        path.write_text(self.assistant(time.time() - 7000, "old"))
        self.change(always=True)
        cache_path = next((self.state / "usage-cache").glob("cache-v2-*.json"))
        with cache_path.with_suffix(".lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            child = subprocess.Popen([str(fixtures.HGS), "recovery", "tick"], env=self.env,
                                     text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                self.wait_for_usage_lock(child.pid)
                with path.open("a") as out:
                    out.write(self.assistant(time.time() - 30, "recent"))
                fcntl.flock(lock, fcntl.LOCK_UN)
                _, stderr = child.communicate(timeout=15)
                self.assertEqual(child.returncode, 0, stderr)
            finally:
                if child.poll() is None:
                    child.terminate()
                    child.communicate(timeout=10)
        after = self.inspect()
        self.assertEqual(after["keep_warm"]["state"], "waiting")
        self.assertFalse(after["session_usage"]["partial"])
        self.assertGreater(after["session_usage"]["prompt_cache"]["expires_at"], time.time())

    def test_an_oversized_record_no_longer_stalls_the_scan(self):
        path = self.transcript()
        with path.open("w") as out:
            out.write(self.assistant(time.time() - 7000, "old"))
            out.write('{"type":"user","message":{"content":"')
            for _ in range(33 * 16):
                out.write("x" * (64 * 1024))
            out.write('"}}\n')
            out.write(self.assistant(time.time() - 3000, "recent"))
        usage = self.inspect()["session_usage"]
        for _ in range(4):
            if not usage["partial"]:
                break
            usage = self.inspect()["session_usage"]
        self.assertFalse(usage["partial"])
        self.assertGreater(usage["prompt_cache"]["expires_at"], time.time())

    def test_a_record_straddling_the_scan_budget_does_not_turn_a_tick_cold(self):
        path = self.transcript()
        old = self.assistant(time.time() - 7000, "old")
        path.write_text(old)
        self.change(always=True)
        # An ordinary row fills the scan to 50 bytes before its budget; the
        # recent assistant row is longer than that, so it straddles the end.
        opening = ('{"type":"user","sessionId":"%s","message":{"content":"'
                   % self.conversation_id)
        closing = '"}}\n'
        target = 32 * 1024 * 1024 - 50 - len(old)
        filler = target - len(opening) - len(closing)
        self.assertGreater(filler, 64 * 1024)
        with path.open("a") as out:
            out.write(opening)
            out.write("x" * filler)
            out.write(closing)
            out.write(self.assistant(time.time() - 30, "recent"))
        self.tick()
        after = self.inspect()
        self.assertNotEqual(after["keep_warm"]["state"], "cold")
        self.assertFalse(after["session_usage"]["partial"])
        self.assertGreater(after["session_usage"]["prompt_cache"]["expires_at"], time.time())

    def test_a_really_expired_cache_still_turns_cold(self):
        path = self.transcript()
        path.write_text(self.assistant(time.time() - 7000, "old"))
        self.change(always=True)
        self.tick()
        after = self.inspect()
        self.assertEqual(after["keep_warm"]["state"], "cold")
        self.assertLess(after["session_usage"]["prompt_cache"]["expires_at"], time.time())

    def test_a_stale_conversation_is_refused(self):
        stale = self.command("keep-warm", self.name, "--json", ok=False, payload=dict(
            expected_run_id=self.run_id, expected_conversation_id="other", expected_revision=0,
            enabled=True, hours=1))
        self.assertEqual(stale["error"], "stale_identity")
        self.assertFalse((self.state / "keep-warm.json").exists())


if __name__ == "__main__":
    unittest.main()
