"""Latest native Claude parses the exact frozen SessionStart JSON, on every OS.

The ordinary resolver supplies one integrity-verified official release. This
control uses only a private home, one read-only output hook and a loopback model.
Whole-entry tests independently compare actual hgs hook stdout with these bytes.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest

import model_api_fixture as api

CLAUDE = os.environ.get("HGS_CLAUDE_TEST_BIN")
HINT = "This agent runs in a Zerus session: the hgs CLI reads, messages, launches and stops other agent sessions on this and other computers. Run hgs guide before using it."
TASKS = "Zerus tasks are this project's shared task list."


@unittest.skipUnless(CLAUDE, "set HGS_CLAUDE_TEST_BIN for the isolated native SessionStart parser")
class NativeClaudeSessionStartJSON(unittest.TestCase):
    def test_one_document_is_structured_context_without_a_hook_parse_error(self):
        with tempfile.TemporaryDirectory(prefix="zerus-session-start-") as directory:
            root = Path(directory)
            home = root / "home"
            config = home / ".claude"
            config.mkdir(parents=True)
            profile = {"hasCompletedOnboarding": True,
                       "customApiKeyResponses": {"approved": ["fixture-key"], "rejected": []}}
            for path in (home / ".claude.json", config / ".claude.json"):
                path.write_text(json.dumps(profile), encoding="utf-8")
            output = root / "context.json"
            output.write_bytes((Path(__file__).parent / "fixtures/session-start-context.json").read_bytes())
            emitter = root / "context.py"
            emitter.write_text("import pathlib, sys\nsys.stdin.buffer.read()\n"
                               "sys.stdout.buffer.write(pathlib.Path(" + repr(str(output))
                               + ").read_bytes())\nsys.stdout.buffer.flush()\n", encoding="utf-8")
            settings = {"hooks": {"SessionStart": [{"hooks": [{"type": "command",
                        "command": sys.executable, "args": ["-I", str(emitter)], "timeout": 5}]}]}}
            (config / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
            server = api.Server(root, str(root), hint=HINT, sentinels=(TASKS,))
            serving = threading.Thread(target=server.serve_forever)
            serving.start()
            try:
                # Keep OS startup essentials, never account/provider/session state.
                env = {key: value for key, value in os.environ.items()
                       if key.upper() in {"PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT"}}
                env.update(HOME=str(home), USERPROFILE=str(home), CLAUDE_CONFIG_DIR=str(config),
                           APPDATA=str(root / "appdata"), LOCALAPPDATA=str(root / "localappdata"),
                           TEMP=str(root), TMP=str(root),
                           ANTHROPIC_API_KEY="fixture-key", ANTHROPIC_BASE_URL=f"http://127.0.0.1:{server.server_port}",
                           CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_AUTOUPDATER="1",
                           DISABLE_UPDATES="1", DISABLE_INSTALLATION_CHECKS="1",
                           CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL="1",
                           DISABLE_TELEMETRY="1", DISABLE_ERROR_REPORTING="1",
                           HTTP_PROXY="http://127.0.0.1:9", HTTPS_PROXY="http://127.0.0.1:9",
                           ALL_PROXY="http://127.0.0.1:9", NO_PROXY="127.0.0.1,localhost")
                debug = root / "debug.log"
                result = subprocess.run([CLAUDE, "-p", "Hello fixture.", "--model", "claude-sonnet-4-6",
                                         "--debug-file", str(debug)], cwd=root, env=env,
                                        stdin=subprocess.DEVNULL, capture_output=True, timeout=60)
                self.assertEqual(result.returncode, 0, (result.stdout + result.stderr).decode(errors="replace"))
                log = debug.read_text(encoding="utf-8", errors="replace")
                self.assertIn("Successfully parsed and validated hook JSON output", log)
                self.assertNotIn("Hook output looks like a JSON object but is not valid JSON", log)
                requests = [request for request in api.requests(root) if request.get("route", "").endswith("/messages")]
                self.assertTrue(requests, "native Claude sent no model request")
                self.assertTrue(any(request.get("hint_count") == 1
                                    and request.get("fixture_sentinels", {}).get(TASKS) == 1 for request in requests),
                                "both contexts must reach the model exactly once")
            finally:
                server.shutdown()
                server.server_close()
                serving.join(timeout=5)
                self.assertFalse(serving.is_alive())
                for worker in server.hint_handlers():
                    worker.join(timeout=5)
                    self.assertFalse(worker.is_alive(), "original model handler did not drain")
