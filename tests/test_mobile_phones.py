#!/usr/bin/env python3
"""hgs mobile: phone pairing and management through the connector credential."""
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import unittest

REPO = Path(__file__).resolve().parents[1]
HGS = Path(os.environ.get("HGS_TEST_BIN", REPO / "target/debug/hgs")).resolve()
DEVICE = "00000000-0000-4000-8000-0000000000d1"

# Records each call and answers from responses.json: {"METHOD path": [status, body]}.
FAKE_CURL = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
root = pathlib.Path(os.environ['MOBILE_FIXTURE'])
config = sys.stdin.read()
args = sys.argv[1:]
method = args[args.index('--request') + 1]
url = args[-1]
with (root / 'calls.jsonl').open('a') as log:
    log.write(json.dumps({'args': args, 'config': config}) + '\n')
responses = json.loads((root / 'responses.json').read_text())
path = url.split('://', 1)[1].split('/', 1)[1]
answer = responses.get(method + ' /' + path)
if answer is None:
    print('curl: (7) Failed to connect', file=sys.stderr)
    sys.exit(7)
status, body = answer
sys.stdout.write(json.dumps(body) + '\n' + str(status))
'''
FAKE_SSH = r'''#!/usr/bin/env python3
import os, pathlib, subprocess, sys
root = pathlib.Path(os.environ['MOBILE_FIXTURE'])
host, command = sys.argv[-2], sys.argv[-1]
home = root / ('remote-' + host)
env = dict(os.environ, HOME=str(home), HGS_SELF=host, HGS_PEERS='',
           HGS_CONFIG_DIR=str(home / 'config'), HGS_STATE_DIR=str(home / 'state'))
sys.exit(subprocess.run(['/bin/sh', '-c', command], env=env).returncode)
'''

FAKE_PROTECTED_CONNECTOR = r'''
import json, os, pathlib, sys
root = pathlib.Path(os.environ['MOBILE_FIXTURE'])
action = sys.argv[2]
json.load(sys.stdin)
with (root / 'protected-calls').open('a') as log:
    log.write(MODE + ':' + action + '\n')
if MODE == 'stale':
    print(json.dumps({'ok': False, 'error': 'stale sibling fixture'}))
    sys.exit(1)
if action == 'status':
    print(json.dumps({'state': 'active', 'encrypted': True, 'server_url': 'https://relay.example.test',
                      'workspace_id': 'fixture', 'workspace_name': 'Fixture', 'config_revision': 'a' * 64}))
elif action == 'list-devices':
    print(json.dumps({'devices': [{'device_id': os.environ.get('PHONE_FIXTURE_DEVICE', ''), 'role': 'phone'}]
                      if os.environ.get('PHONE_FIXTURE_DEVICE') else []}))
elif action == 'invite-phone':
    print(json.dumps({'code': 'ze1.synthetic-invitation'}))
elif action == 'revoke-device':
    print(json.dumps({}))
else:
    raise AssertionError(action)
'''


class MobilePhones(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hgs-mobile-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "home"
        self.config = self.root / "config"
        self.bin = self.home / ".local/bin"
        for path in (self.home, self.config, self.root / "state", self.bin):
            path.mkdir(parents=True, exist_ok=True)
        for name, text in (("curl", FAKE_CURL), ("ssh", FAKE_SSH)):
            (self.bin / name).write_text(text)
            (self.bin / name).chmod(0o755)
        self.respond({})
        self.env = dict(os.environ, HOME=str(self.home), MOBILE_FIXTURE=str(self.root),
                        HGS_CONFIG_DIR=str(self.config), HGS_STATE_DIR=str(self.root / "state"),
                        HGS_SELF="local", HGS_PEERS="peer", HGS_TAB="0",
                        PATH=str(self.bin) + ":" + os.environ["PATH"])
        self.env.pop("HGS_MOBILE_ALLOW_INSECURE_LOCALHOST", None)

    def connect(self, config_dir=None, server="https://relay.example.test/"):
        directory = (config_dir or self.config) / "mobile"
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "connector.json").write_text(json.dumps(
            {"server_url": server, "node_token": "synthetic-node-token", "state_dir": "~/state"}))

    def respond(self, responses):
        (self.root / "responses.json").write_text(json.dumps(responses))

    def calls(self):
        log = self.root / "calls.jsonl"
        return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

    def hgs(self, *args, env=None):
        return subprocess.run([str(HGS), *args], env=env or self.env, capture_output=True, text=True, timeout=30)

    def test_a_computer_without_a_mobile_connection_reports_it(self):
        result = self.hgs("mobile", "phones", "--json")
        self.assertEqual((result.returncode, json.loads(result.stdout)), (0, {"configured": False}))
        result = self.hgs("mobile", "invite", "--json")
        self.assertEqual(result.returncode, 1)
        self.assertIn("mobile connection", result.stderr)
        self.assertEqual(self.calls(), [])

    @unittest.skipIf(os.name == 'nt', 'Unix executable fixtures; Windows runs the exact packaged UI trial')
    def test_inherited_installer_booleans_do_not_select_a_runtime_connector(self):
        package = self.root / 'installer-flag-package'
        package.mkdir()
        staged = package / 'hgs'
        try:
            os.link(HGS, staged)
        except OSError:
            shutil.copy2(HGS, staged)
        connector = package / 'zerus-mobile-connector'
        connector.write_text('#!/usr/bin/env python3\nMODE="selected"\n' + FAKE_PROTECTED_CONNECTOR)
        connector.chmod(0o755)
        folder = self.config / 'mobile'
        folder.mkdir()
        (folder / 'connector.json').write_text(json.dumps(
            {'server_url': 'https://relay.example.test', 'encrypted_workspace': {}}))
        for flag in ['0', '1']:
            with self.subTest(installer_flag=flag):
                environment = dict(self.env, HGS_MOBILE_CONNECTOR=flag)
                environment.pop('ZERUS_CLIENT_MOBILE_CONNECTOR_PATH', None)
                result = subprocess.run([str(staged), 'mobile', 'phones', '--json'], env=environment,
                                        capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(json.loads(result.stdout)['configured'])

    @unittest.skipIf(os.name == 'nt', 'Unix executable fixtures; Windows runs the exact packaged UI trial')
    def test_exact_packaged_connector_beats_a_stale_sibling_of_the_real_staged_cli(self):
        stage, package = self.root / 'stage', self.root / 'package'
        stage.mkdir()
        package.mkdir()
        staged = stage / 'hgs'
        try:
            os.link(HGS, staged)
        except OSError:
            shutil.copy2(HGS, staged)
        selected = package / 'zerus-mobile-connector'
        for directory, mode in [(stage, 'stale'), (package, 'selected')]:
            program = directory / 'zerus-mobile-connector'
            program.write_text('#!/usr/bin/env python3\nMODE=' + repr(mode) + '\n' + FAKE_PROTECTED_CONNECTOR)
            program.chmod(0o755)
        folder = self.config / 'mobile'
        folder.mkdir()
        (folder / 'connector.json').write_text(json.dumps(
            {'server_url': 'https://relay.example.test', 'encrypted_workspace': {}}))
        environment = dict(self.env, PATH=str(package) + os.pathsep + self.env['PATH'])
        environment.pop('ZERUS_CLIENT_MOBILE_CONNECTOR_PATH', None)

        def run():
            return subprocess.run([str(staged), 'mobile', 'phones', '--json'], env=environment,
                                  capture_output=True, text=True, timeout=15)

        # Reproduce the actual competing-sibling refusal even with the package first on PATH.
        old = run()
        self.assertEqual(old.returncode, 1, old.stderr)
        self.assertIn('stale sibling fixture', old.stderr)
        calls = self.root / 'protected-calls'
        self.assertEqual(calls.read_text().splitlines(), ['stale:status'])
        calls.write_text('')
        environment['ZERUS_CLIENT_MOBILE_CONNECTOR_PATH'] = str(selected)
        current = run()
        self.assertEqual(current.returncode, 0, current.stderr)
        self.assertTrue(json.loads(current.stdout)['configured'])
        self.assertEqual(calls.read_text().splitlines(), ['selected:status', 'selected:list-devices'])
        calls.write_text('')
        for bad in ['', 'relative-connector', str(package / 'missing-connector')]:
            environment['ZERUS_CLIENT_MOBILE_CONNECTOR_PATH'] = bad
            refused = run()
            self.assertNotEqual(refused.returncode, 0)
            self.assertEqual(calls.read_text(), '', 'invalid explicit selection must never fall back')

    @unittest.skipIf(os.name == 'nt', 'Unix executable fixtures; Windows runs the exact packaged UI trial')
    def test_the_selected_connector_serves_every_local_phone_action_of_the_real_staged_cli(self):
        # zerus-ihrzs: the desktop passes the connector its status probe used.
        # The real staged CLI must run exactly that program for phones, invite
        # and revoke-phone, whatever sits beside it or first on PATH.
        package, on_path = self.root / 'package', self.root / 'on-path'
        competing, lone = self.root / 'competing', self.root / 'lone'
        for directory in (package, on_path, competing, lone):
            directory.mkdir()
            if directory in (competing, lone):
                try:
                    os.link(HGS, directory / 'hgs')
                except OSError:
                    shutil.copy2(HGS, directory / 'hgs')
        for directory, mode in [(package, 'selected'), (on_path, 'stale'), (competing, 'stale')]:
            program = directory / 'zerus-mobile-connector'
            program.write_text('#!/usr/bin/env python3\nMODE=' + repr(mode) + '\n' + FAKE_PROTECTED_CONNECTOR)
            program.chmod(0o755)
        selected = package / 'zerus-mobile-connector'
        folder = self.config / 'mobile'
        folder.mkdir()
        (folder / 'connector.json').write_text(json.dumps(
            {'server_url': 'https://relay.example.test', 'encrypted_workspace': {}}))
        actions = {
            'phones': (['mobile', 'phones', '--json'], ['status', 'list-devices']),
            'invite': (['mobile', 'invite', '--json'], ['status', 'invite-phone']),
            'revoke-phone': (['mobile', 'revoke-phone', DEVICE, '--json'], ['status', 'list-devices', 'revoke-device']),
        }
        calls = self.root / 'protected-calls'

        def run(stage, args, selection):
            environment = dict(self.env, PHONE_FIXTURE_DEVICE=DEVICE,
                               PATH=str(on_path) + os.pathsep + self.env['PATH'])
            environment.pop('ZERUS_CLIENT_MOBILE_CONNECTOR_PATH', None)
            if selection is not None:
                environment['ZERUS_CLIENT_MOBILE_CONNECTOR_PATH'] = selection
            calls.write_text('')
            result = subprocess.run([str(stage / 'hgs'), *args], env=environment,
                                    capture_output=True, text=True, timeout=15)
            return result, calls.read_text().splitlines()

        for stage, name in [(competing, 'competing refusing sibling'), (lone, 'missing sibling, stale PATH')]:
            for action, (args, sequence) in actions.items():
                with self.subTest(stage=name, action=action, selection='none'):
                    # Without a selection the CLI keeps its order: the sibling,
                    # else PATH. Both reach the refusing connector here.
                    result, seen = run(stage, args, None)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn('stale sibling fixture', result.stderr)
                    self.assertEqual(seen, ['stale:status'])
                with self.subTest(stage=name, action=action, selection='exact'):
                    result, seen = run(stage, args, str(selected))
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertTrue(json.loads(result.stdout)['encrypted'])
                    self.assertEqual(seen, ['selected:' + step for step in sequence])
                for bad in ['', 'relative-connector', str(package / 'missing-connector'), str(package)]:
                    with self.subTest(stage=name, action=action, selection=bad or 'empty'):
                        result, seen = run(stage, args, bad)
                        self.assertNotEqual(result.returncode, 0)
                        self.assertEqual(seen, [], 'an invalid selection must fail closed, never fall back')

    def test_phones_come_from_the_relay_with_the_token_kept_off_the_command_line(self):
        self.connect()
        devices = {"workspace_id": "w", "workspace_name": "Example workspace", "devices": [
            {"id": DEVICE, "name": "Zerus Android", "platform": "android", "paired_at": 1.0,
             "last_seen_at": 2.0, "added_by": None}]}
        self.respond({"GET /v1/node/devices": [200, devices]})
        result = self.hgs("mobile", "phones", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout),
                         {"configured": True, "server_url": "https://relay.example.test", **devices})
        [call] = self.calls()
        self.assertEqual(call["args"][-1], "https://relay.example.test/v1/node/devices")
        self.assertNotIn("synthetic-node-token", " ".join(call["args"]))
        self.assertIn('header = "Authorization: Bearer synthetic-node-token"', call["config"])
        for flag in ("--noproxy", "--proto", "--max-time", "--connect-timeout"):
            self.assertIn(flag, call["args"])
        # curl ignores its default configuration only when -q comes first.
        self.assertEqual(call["args"][0], "-q")
        self.assertEqual(call["args"][call["args"].index("--proto") + 1], "=https")

    def test_an_invitation_carries_the_pairing_link(self):
        self.connect()
        self.respond({"POST /v1/node/invitations": [200, {
            "workspace_id": "w", "workspace_name": "Example workspace",
            "pair_code": "synthetic_pair-code_0123456789", "expires_at": 1000.5}]})
        result = self.hgs("mobile", "invite", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        invitation = json.loads(result.stdout)
        self.assertEqual(invitation["pair_url"],
                         "zerus://pair?server=https%3A%2F%2Frelay.example.test&code=synthetic_pair-code_0123456789")
        self.assertEqual((invitation["server_url"], invitation["expires_at"]), ("https://relay.example.test", 1000.5))
        [call] = self.calls()
        self.assertEqual(call["args"][call["args"].index("--data") + 1], "{}")

    def test_revoking_a_phone_names_one_exact_device(self):
        self.connect()
        self.respond({"DELETE /v1/node/devices/" + DEVICE: [200, {"revoked": True}]})
        result = self.hgs("mobile", "revoke-phone", DEVICE, "--json")
        self.assertEqual((result.returncode, json.loads(result.stdout)),
                         (0, {"revoked": True, "device_id": DEVICE}), result.stderr)
        result = self.hgs("mobile", "revoke-phone", "../devices", "--json")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(self.calls()), 1)

    def test_relay_failures_are_explained(self):
        self.connect()
        self.respond({"GET /v1/node/devices": [404, {"error": "not found"}],
                      "POST /v1/node/invitations": [429, {"error": "relay capacity is busy"}],
                      "DELETE /v1/node/devices/" + DEVICE: [401, {"error": "credential not authorized"}]})
        for args, message in ((("phones",), "does not support phone management"),
                              (("invite",), "Too many"),
                              (("revoke-phone", DEVICE), "rejected this computer")):
            result = self.hgs("mobile", *args, "--json")
            self.assertEqual(result.returncode, 1)
            self.assertIn(message, result.stderr)
        self.respond({})
        result = self.hgs("mobile", "phones", "--json")
        self.assertEqual(result.returncode, 1)
        self.assertIn("Could not reach the relay", result.stderr)

    def test_plain_http_is_only_for_an_explicit_loopback_test_relay(self):
        self.connect(server="http://127.0.0.1:8787")
        self.respond({"GET /v1/node/devices": [200, {"devices": []}]})
        result = self.hgs("mobile", "phones", "--json")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self.calls(), [])
        result = self.hgs("mobile", "phones", "--json", env={**self.env, "HGS_MOBILE_ALLOW_INSECURE_LOCALHOST": "1"})
        self.assertEqual(result.returncode, 0, result.stderr)
        [call] = self.calls()
        self.assertEqual(call["args"][call["args"].index("--proto") + 1], "=http,https")

    def test_invite_and_revoke_stay_with_the_relay_and_workspace_that_were_listed(self):
        self.connect()
        pin = ["--workspace", "w1", "--server", "https://relay.example.test"]
        invitation = {"workspace_id": "w1", "workspace_name": "Example workspace",
                      "pair_code": "synthetic_pair-code_0123456789", "expires_at": 1000.5}
        self.respond({"POST /v1/node/invitations": [200, invitation],
                      "DELETE /v1/node/devices/" + DEVICE + "?workspace_id=w1": [200, {"revoked": True}]})
        result = self.hgs("mobile", "invite", "--workspace", "w1", "--server", "https://other.example.test", "--json")
        self.assertEqual(result.returncode, 1)
        self.assertIn("another relay", result.stderr)
        self.assertEqual(self.calls(), [])
        result = self.hgs("mobile", "invite", *pin, "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        [call] = self.calls()
        self.assertEqual(json.loads(call["args"][call["args"].index("--data") + 1]), {"workspace_id": "w1"})
        result = self.hgs("mobile", "revoke-phone", DEVICE, *pin, "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.calls()[-1]["args"][-1].endswith("/v1/node/devices/" + DEVICE + "?workspace_id=w1"))
        # A relay answering for another workspace never yields a usable code.
        self.respond({"POST /v1/node/invitations": [200, {**invitation, "workspace_id": "w2"}]})
        result = self.hgs("mobile", "invite", *pin, "--json")
        self.assertEqual((result.returncode, result.stdout), (1, ""))
        self.assertIn("another workspace", result.stderr)
        self.respond({"POST /v1/node/invitations": [409, {"error": "request identity or route conflicts"}]})
        result = self.hgs("mobile", "invite", *pin, "--json")
        self.assertEqual(result.returncode, 1)
        self.assertIn("another workspace", result.stderr)

    @unittest.skipUnless(shutil.which("curl"), "needs the system curl")
    def test_a_hostile_curlrc_cannot_redirect_or_trace_the_credential(self):
        (self.bin / "curl").unlink()
        seen = []

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                seen.append((self.server.server_port, self.headers.get("Authorization")))
                if self.server is relay:
                    self.send_response(302)
                    self.send_header("Location", "http://127.0.0.1:%d/v1/node/devices" % other.server_port)
                    self.end_headers()
                else:
                    body = json.dumps({"devices": []}).encode()
                    self.send_response(200)
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)

        relay, other = HTTPServer(("127.0.0.1", 0), Handler), HTTPServer(("127.0.0.1", 0), Handler)
        for server in (relay, other):
            threading.Thread(target=server.serve_forever, daemon=True).start()
            self.addCleanup(server.server_close)
            self.addCleanup(server.shutdown)
        trace = self.root / "trace.txt"
        (self.home / ".curlrc").write_text("location\ntrace = \"%s\"\n" % trace)
        self.connect(server="http://127.0.0.1:%d" % relay.server_port)
        result = self.hgs("mobile", "phones", "--json",
                          env={**self.env, "CURL_HOME": str(self.home), "HGS_MOBILE_ALLOW_INSECURE_LOCALHOST": "1"})
        self.assertEqual(result.returncode, 1)
        self.assertIn("HTTP 302", result.stderr)
        self.assertEqual(seen, [(relay.server_port, "Bearer synthetic-node-token")])
        self.assertFalse(trace.exists())
        self.assertNotIn("synthetic-node-token", result.stderr)

    def test_a_peer_manages_phones_through_its_gateway(self):
        remote = self.root / "remote-peer/.local/bin"
        remote.mkdir(parents=True)
        (remote / "hgs").symlink_to(HGS)
        (remote / "curl").symlink_to(self.bin / "curl")
        self.connect(self.root / "remote-peer/config")
        self.respond({"GET /v1/node/devices": [200, {"devices": []}]})
        result = self.hgs("@peer", "mobile", "phones", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["configured"], True)


if __name__ == "__main__":
    unittest.main()
