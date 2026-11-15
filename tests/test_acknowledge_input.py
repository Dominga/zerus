"""Person recovery of an exact hook-silent input. Private tmux, home and agent only."""
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import time
import unittest
import uuid

import test_input as fixture


@unittest.skipUnless(sys.platform.startswith(("linux", "darwin")), "Linux and macOS person recovery")
class AcknowledgeInput(unittest.TestCase):
    def setUp(self):
        self.f = fixture.InputTransport("test_an_emptied_claude_composer_alone_never_confirms_the_input")
        tmux = self.f.tmux
        def tracked_agent(*args, **kwargs):
            if args[0] == "new-session" and "-s" in args and args[args.index("-s") + 1] == self.f.name:
                args = (args[0], "-e", "HGS_SESSION=" + self.f.name,
                        "-e", "HGS_RUN_ID=" + self.f.run_id, *args[1:])
            return tmux(*args, **kwargs)
        self.f.tmux = tracked_agent
        self.f.setUp()
        self.addCleanup(self.f.doCleanups)
        self.addCleanup(self.f.tearDown)
        # Keep the tracked agent's original markers in its native environment,
        # while the separate person terminal inherits none from this session.
        for key in ("HGS_SESSION", "HGS_RUN_ID"):
            self.f.tmux("set-environment", "-u", "-t", "=" + self.f.name + ":", key)
        self.f.claude_ready()
        self.f.record.pop("transcript")
        self.f.write_record()
        self.original = self.f.payload(text="Synthetic original message")
        self.receipt = self.f.send(self.original)
        self.assertEqual(self.receipt["native"], "composer")
        self.pending = json.loads(self.f.record_path.read_text())
        self.before = self.f.received()

    def request(self, **changes):
        state = self.f.terminal_request(action="composer")
        pending = self.pending
        request = dict(request_id=str(uuid.uuid4()), name=self.f.name,
                       expected_run_id=self.f.run_id, expected_conversation_id=self.f.conversation_id,
                       original_request_id=self.original["request_id"],
                       pending_at=pending["input_pending_at"],
                       terminal_binding_id=state["terminal_binding_id"], expires_at=time.time() + 5)
        request.update(changes)
        return request

    def execute(self, request, person=True, env_patch=None):
        """A foreground terminal outside the private agent, or a child of that agent.

        The private tmux daemon owns both terminals; no real agent or owner server
        participates. The child case clears session markers, preserving ancestry.
        """
        slot = self.f.root / str(uuid.uuid4())
        slot.mkdir()
        (slot / "request.json").write_text(json.dumps(request))
        env = {key: value for key, value in self.f.env.items()
               if not key.startswith(("HGS_SESSION", "HGS_RUN_ID", "CODEX_", "CLAUDE_", "KIMI_"))}
        env.update(env_patch or {})
        script = slot / "invoke.py"
        script.write_text(
            "import json,subprocess\nfrom pathlib import Path\n"
            f"p=Path({str(slot)!r})\n"
            f"r=subprocess.run({[str(fixture.HGS), 'acknowledge-input', self.f.name, '--json']!r},"
            f"input=(p/'request.json').read_text(),text=True,capture_output=True,env={env!r},timeout=10)\n"
            "(p/'result.json').write_text(json.dumps(dict(exit=r.returncode,stdout=r.stdout,stderr=r.stderr)))\n")
        if person:
            self.f.tmux("new-session", "-d", "-s", "person-review-" + uuid.uuid4().hex, "python3", str(script))
        else:
            (self.f.root / "nested-launch.json").write_text(json.dumps(dict(
                argv=["python3", str(script)], env=env)))
            os.kill(self.f.record["pid"], signal.SIGUSR2)
        self.f.wait_until(lambda: (slot / "result.json").exists(), 15)
        result = json.loads((slot / "result.json").read_text())
        self.assertEqual(self.f.received(), self.before, "acknowledgment must type zero keys")
        return result

    def test_unknown_claude_input_cannot_be_confirmed_by_a_later_identical_transcript_row(self):
        result=self.execute(self.request());self.assertEqual(result['exit'],0,result)
        self.f.record=json.loads(self.f.record_path.read_text())
        self.f.claude_ready()
        following=self.f.payload(text=self.original['text'])
        result=self.f.slow_send(following)
        self.assertNotEqual(result.returncode,0,result.stdout)
        self.assertIn('delivery uncertain',result.stderr)
        current=json.loads(self.f.record_path.read_text())
        self.assertLess(current['input_seen_at'],current['input_pending_at'])
        self.assertEqual(self.f.claude_rows()[-1]['message']['content'],self.receipt['submitted_text'])
        self.assertEqual(json.loads((self.f.state/'input_receipts'/(self.original['request_id']+'.json')).read_text()),self.receipt)
        self.assertEqual(self.f.received()[len(self.before):].count(b'\r'),1)

    def test_kimi_completion_requires_an_actual_integer_turn_identity(self):
        self.f.record=dict(self.pending,agent='kimi',activity='busy',phase='working')
        self.f.screen('\x1b[2J\x1b[H ╭──────────────────────────────╮\r\n │ >                            │\r\n ╰──────────────────────────────╯\r\n Never Ask  fixture\x1b[2;6H','5:1')
        session=self.f.root/'.kimi-code/sessions/workspace'/self.f.conversation_id
        wire=session/'agents/main/wire.jsonl';wire.parent.mkdir(parents=True)
        (session/'state.json').write_text(json.dumps(dict(id=self.f.conversation_id,version=2)))
        at=(self.pending['input_pending_at']-10)*1000
        for turn in (False,True,{},[],'0',None,-1,2**63,0,1):
            with self.subTest(turn=turn):
                rows=[dict(type='turn.prompt',agentId='main',turnId=turn,time=at,origin=dict(kind='user'),input=[dict(type='text',text='Old native turn')]),
                      dict(type='turn.ended',agentId='main',turnId=turn,time=at+1,reason='completed')]
                wire.write_text(''.join(json.dumps(row)+'\n' for row in rows));self.f.write_record()
                composer=self.f.terminal_request(action='composer')['composer']
                self.assertEqual('recovery' in composer,type(turn) is int and 0<=turn<2**63,composer)

    @unittest.skipUnless(sys.platform == "linux", "Private D-Bus service fixture; Mac uses foreground CLI and typed frontend tests")
    def test_original_native_frontend_authority_reaches_the_real_service_worker(self):
        # Run the maintained native frontend fixture in an independent private
        # terminal, so its positive caller has no surrounding agent ancestry.
        request = self.request()
        self.assertIsNotNone(shutil.which("dbus-run-session"), "The native service fixture requires a private bus")
        self.f.state.chmod(0o700)
        slot = self.f.root / "native-review"
        slot.mkdir()
        script = slot / "native.py"
        tmux_script = '#!/bin/sh\nexec ' + str(self.f.bin / 'tmux') + ' "$@"\n'
        script.write_text(
            "import json,os,sys,time,traceback\nfrom pathlib import Path\n"
            "assert not any(os.environ.get(k) for k in ('DISPLAY','WAYLAND_DISPLAY','XAUTHORITY'))\n"
            "assert os.environ.get('DBUS_SESSION_BUS_ADDRESS','').startswith('unix:')\n"
            f"sys.path.insert(0,{str(Path(__file__).resolve().parent)!r})\n"
            "import test_desktop_activation as native\n"
            "f=native.Activation('test_authenticated_activation_read_reaches_real_receiver_and_returns_idle_reply')\n"
            "f.setUp()\n"
            "failure=None\n"
            "try:\n"
            f" f.env['HGS_STATE_DIR']={str(self.f.state)!r}\n"
            f" f.state=Path({str(self.f.state)!r}); f.desktop=f.state/'desktop'\n"
            " f.env['HGS_PEERS']=''\n"
            f" (f.root/'.local/bin/tmux').write_text({tmux_script!r})\n"
            f" ack={request!r}; ack['expires_at']=time.time()+5\n"
            " frontend,_,_=f.begin(acknowledgment=ack)\n"
            " reply,error=f.result(frontend)\n"
            " assert reply['status']=='person-acknowledged-unknown',repr(reply)\n"
            " raw=f.cli('desktop','request','--json',data=dict(op='input_acknowledge',acknowledgment=ack))\n"
            " assert raw['accepted'] is False,repr(raw)\n"
            " lookup=f.cli('desktop','request','--json',data=dict(op='input_acknowledge_result',request_id=ack['original_request_id'],acknowledgment=ack))\n"
            " assert lookup['found'] is True and lookup['result']==reply,repr(lookup)\n"
            "except BaseException:\n"
            " failure=traceback.format_exc()\n"
            "finally:\n"
            " if not f.doCleanups(): failure=(failure or '')+'Native fixture cleanup failed'\n"
            " if failure:\n"
            f"  Path({str(slot / 'error')!r}).write_text(failure)\n"
            " else:\n"
            f"  Path({str(slot / 'result.json')!r}).write_text(json.dumps(reply))\n")
        runtime = slot / "runtime"
        runtime.mkdir(mode=0o700)
        self.f.tmux("new-session", "-d", "-s", "native-person-" + uuid.uuid4().hex,
                    "env", "-u", "DISPLAY", "-u", "WAYLAND_DISPLAY", "-u", "XAUTHORITY",
                    "-u", "DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR=" + str(runtime),
                    "XDG_SESSION_TYPE=unspecified", "dbus-run-session", "--", "python3", str(script))
        self.f.wait_until(lambda: (slot / "result.json").exists() or (slot / "error").exists(), 30)
        self.assertFalse((slot / "error").exists(), (slot / "error").read_text() if (slot / "error").exists() else "")
        self.assertEqual(self.f.received(), self.before)
        current = json.loads(self.f.record_path.read_text())
        self.assertEqual(current["input_acknowledged_unknown"]["source"], "native_frontend")

    def test_person_releases_only_the_original_input_without_claiming_acceptance_or_replay(self):
        request = self.request()
        result = self.execute(request)
        self.assertEqual(result["exit"], 0, result)
        answer = json.loads(result["stdout"])
        self.assertEqual(answer["status"], "person-acknowledged-unknown")
        self.assertEqual(answer["original_request_id"], self.original["request_id"])
        current = json.loads(self.f.record_path.read_text())
        self.assertEqual(current["input_seen_at"], self.pending["input_seen_at"])
        self.assertEqual(current["input_pending_request"], self.original["request_id"])
        self.assertNotIn("input_cleared_at", current)
        receipt_path = self.f.state / "input_receipts" / (self.original["request_id"] + ".json")
        self.assertEqual(json.loads(receipt_path.read_text()), self.receipt)
        self.assertEqual(json.loads(self.execute(request)["stdout"]), answer)
        self.assertEqual(self.f.send(self.original), self.receipt)
        self.assertEqual(self.f.received(), self.before, "original send UUID is never typed again")
        following = self.f.send(self.f.payload(text="A deliberately new message"))
        self.assertEqual(following["native"], "composer")
        self.before = self.f.received()
        self.assertEqual(json.loads(self.execute(request)["stdout"]), answer)
        self.assertIn("previous input has not been confirmed", self.f.send(self.f.payload(text="third"), success=False))

    def provider_recovery(self, provider, busy=False):
        self.f.record = dict(self.pending, agent=provider,
            activity='busy' if busy else 'idle', phase='working' if busy else 'idle')
        if provider == 'codex':
            self.f.screen('\x1b[2J\x1b[H\x1b[1m›\x1b[0m ', '2:0')
        else:
            self.f.screen('\x1b[2J\x1b[H ╭──────────────────────────────╮\r\n │ >                            │\r\n ╰──────────────────────────────╯\r\n Never Ask  fixture\x1b[2;6H', '5:1')
        if busy:
            session = self.f.root / '.kimi-code/sessions/workspace' / self.f.conversation_id
            wire = session / 'agents/main/wire.jsonl'
            wire.parent.mkdir(parents=True)
            (session / 'state.json').write_text(json.dumps(dict(id=self.f.conversation_id, version=2)))
            at = (self.pending['input_pending_at'] - 10) * 1000
            rows = [dict(type='turn.prompt', agentId='main', turnId=1, promptId='msg_old', time=at,
                         origin=dict(kind='user'), input=[dict(type='text', text='Previous native turn')]),
                    dict(type='turn.ended', agentId='main', turnId=1, time=at+1, reason='completed')]
            wire.write_text(''.join(json.dumps(row)+'\n' for row in rows))
        self.f.write_record()
        self.assertIn('recovery', self.f.terminal_request(action='composer')['composer'])
        request = self.request()
        self.assertNotEqual(self.execute(request, person=False)['exit'], 0)
        result = self.execute(request)
        self.assertEqual(result['exit'], 0, result)
        self.assertEqual(json.loads(result['stdout'])['status'], 'person-acknowledged-unknown')
        current = json.loads(self.f.record_path.read_text())
        self.assertEqual(current['input_seen_at'], self.pending['input_seen_at'])
        self.assertEqual(current['input_pending_at'], self.pending['input_pending_at'])
        self.assertEqual(self.f.received(), self.before)
        self.f.composer_for(provider)
        following = self.f.send(self.f.payload(text='Deliberately new provider input'))
        self.assertEqual(following['status'], 'submitted')
        self.before = self.f.received()
        # Repeating the old person UUID returns its old receipt and must never
        # settle the deliberately new pending input.
        self.assertEqual(json.loads(self.execute(request)['stdout'])['status'], 'person-acknowledged-unknown')
        self.assertIn('previous input has not been confirmed', self.f.send(self.f.payload(text='third provider input'), success=False))

    def test_person_can_release_hookless_codex_without_native_acceptance(self):
        self.provider_recovery('codex')

    def test_person_can_release_hookless_kimi_without_native_acceptance(self):
        self.provider_recovery('kimi')

    def test_person_can_review_kimi_native_completed_with_stale_busy_cache(self):
        self.provider_recovery('kimi', busy=True)

    def test_an_agent_cannot_release_input_even_after_clearing_session_markers(self):
        result = self.execute(self.request(), person=False)
        self.assertNotEqual(result["exit"], 0)
        self.assertIn("agent", result["stderr"].lower())
        self.assertIn("previous input has not been confirmed", self.f.send(self.f.payload(), success=False))

    def test_busy_nonempty_unknown_and_copy_mode_never_offer_or_apply_release(self):
        request = self.request()
        self.f.record = dict(self.pending, activity="busy", phase="working")
        self.f.write_record()
        self.assertNotEqual(self.execute(request)["exit"], 0)
        self.assertNotIn("recovery", self.f.terminal_request(action="composer")["composer"])
        self.f.record = dict(self.pending)
        self.f.write_record()
        for screen, cursor in [("\x1b[2J\x1b[H❯ draft", "7:0"), ("\x1b[2J\x1b[HPassword: ", "10:0")]:
            self.f.screen(screen, cursor)
            self.assertNotEqual(self.execute(request)["exit"], 0)
            self.assertNotIn("recovery", self.f.terminal_request(action="composer")["composer"])
        self.f.screen("\x1b[2J\x1b[H❯ ", "2:0")
        self.f.tmux("copy-mode", "-t", self.f.pane)
        self.assertNotEqual(self.execute(request)["exit"], 0)
        self.assertNotIn("recovery", self.f.terminal_request(action="composer")["composer"])

    def test_an_active_proof_window_and_missing_conversation_identity_stay_closed(self):
        request = self.request()
        receipt_path = self.f.state / "input_receipts" / (self.original["request_id"] + ".json")
        receipt_path.write_text(json.dumps(dict(self.receipt, status="in_progress")))
        self.assertNotEqual(self.execute(request)["exit"], 0)
        self.assertNotIn("recovery", self.f.terminal_request(action="composer")["composer"])
        receipt_path.write_text(json.dumps(self.receipt))
        self.f.record = dict(self.pending, conversation_id=None, conversation_state="unknown")
        self.f.write_record()
        self.assertNotEqual(self.execute(dict(request, expected_conversation_id=""))["exit"], 0)
        self.assertIn("identity is unconfirmed", self.f.send(self.f.payload(expected_conversation_id=""), success=False))

    def test_a_reserved_unknown_decision_is_never_replayed(self):
        request = self.request()
        directory = self.f.state / "input_acknowledgments"
        directory.mkdir(mode=0o700)
        path = directory / (request["request_id"] + ".json")
        path.write_text(json.dumps(dict(op="input_acknowledge", request_id=request["request_id"],
                                       status="claimed", acknowledgment=request, accepted=None, durable=False,
                                       original_request_id=request["original_request_id"], name=self.f.name,
                                       run_id=self.f.run_id, conversation_id=self.f.conversation_id,
                                       source="interactive_terminal")))
        original = path.read_bytes()
        self.assertNotEqual(self.execute(request)["exit"], 0)
        self.assertEqual(path.read_bytes(), original)
        current = json.loads(self.f.record_path.read_text())
        self.assertNotIn("input_acknowledged_unknown", current)
        self.assertIn("previous input has not been confirmed", self.f.send(self.f.payload(), success=False))

    def test_a_malformed_recorded_outcome_is_never_reported_as_person_acknowledged(self):
        request = self.request()
        directory = self.f.state / "input_acknowledgments"
        directory.mkdir(mode=0o700)
        path = directory / (request["request_id"] + ".json")
        receipt = dict(op="input_acknowledge", request_id=request["request_id"], original_request_id=request["original_request_id"],
                       name=self.f.name, run_id=self.f.run_id, conversation_id=self.f.conversation_id,
                       status="person-acknowledged-unknown", acknowledgment=request,
                       accepted=True, durable=True, source="interactive_terminal", settled_at=time.time())
        for patch in [dict(accepted=False), dict(durable=False), dict(run_id="changed"),
                      dict(original_request_id=str(uuid.uuid4())), dict(settled_at=None), dict(source="agent")]:
            with self.subTest(patch=patch):
                path.write_text(json.dumps(dict(receipt, **patch)))
                before = path.read_bytes()
                result = self.execute(request)
                self.assertNotEqual(result["exit"], 0, "a status word alone is not a completed audited decision")
                self.assertEqual(path.read_bytes(), before)
        self.assertNotIn("input_acknowledged_unknown", json.loads(self.f.record_path.read_text()))

    def test_changed_original_identity_binding_and_deadline_refuse_without_keys(self):
        for patch in [dict(expected_run_id="changed"), dict(expected_conversation_id="changed"),
                      dict(original_request_id=str(uuid.uuid4())), dict(pending_at=1.0),
                      dict(terminal_binding_id="0" * 64), dict(expires_at=time.time() - 1)]:
            with self.subTest(patch=patch):
                result = self.execute(self.request(**patch))
                self.assertNotEqual(result["exit"], 0, result)
        self.assertIn("previous input has not been confirmed", self.f.send(self.f.payload(), success=False))


if __name__ == "__main__":
    unittest.main()
