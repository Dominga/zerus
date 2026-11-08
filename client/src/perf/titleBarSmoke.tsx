// Task 243: the Compact skins' one title bar over the sessions page, the list
// shown and hidden (`?strip=1` starts it hidden, as a saved collapse), beside
// Qt's whiteWhWorkspaceView preview; `?skin=zerus-dark` keeps the rail.
// Synthetic: no CLI; the only IPC answered is the settings read.
//   http://127.0.0.1:<port>/scripts/title-bar-smoke.html?skin=whitewh-dark[&strip=1]
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string) => {
    if (command === "settings_get") return { values: {}, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    if (command === "window_control") return false;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import { Show } from "solid-js";
import { render } from "solid-js/web";
import { TitleBar } from "../ui/components/TitleBar";
import { Rail } from "../ui/components/Rail";
import { SessionList } from "../ui/sessions/SessionList";
import { applyBatch } from "../store/catalog";
import "../store/session";
import { applySkin, skin } from "../skin/runtime";
import { setUnifiedChrome } from "../store/ui";
import { emptyBatch, type Batch, type ViewRow } from "../types";

const t0 = 1793050800;
const row = (n: number, title: string, status: string, extra: Partial<ViewRow> = {}): ViewRow => ({ name: `codex/orbit/session-${n}`, title, agent: n % 2 ? "claude" : "codex", cmd: "codex",
  machine: "workstation", project: "orbit", folder: `/work/orbit/${n}`, cwd: `/work/orbit/${n}`, branch: "main", state: "running", tracked: true, run_id: `run-${n}`,
  conversation_id: `conversation-${n}`, created: t0 + n, status, activity: status === "working" ? "busy" : "idle", updated_at: t0 + 100 + n, summary: `Session ${n} summary`, ...extra } as ViewRow);

async function run() {
  await document.fonts.ready;
  applySkin(skin());
  const unified = () => skin().layout.chrome === "unified";
  setUnifiedChrome(unified());
  render(() => (
    <div class="app" classList={{ "chrome-unified": unified() }} style={{ height: "100vh" }}>
      <Show when={unified()} fallback={<Rail />}><TitleBar /></Show>
      <main class="page"><div class="sessions-page"><SessionList /><div style={{ flex: "1", display: "grid", "place-items": "center", color: "var(--z-color-muted)" }}>Conversation</div></div></main>
      <footer class="statusbar">workstation, 6 sessions</footer>
    </div>
  ), document.getElementById("root")!);
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [
    row(1, "Saved cards API", "working"), row(2, "Card form UI", "ready", { unread: true }), row(3, "Contract tests", "attention", { attention: "approval", phase: "approval" }),
    row(4, "Release notes", "ready"), row(5, "Checkout lead", "working"), row(6, "Load test", "ready"),
  ].map((r, i) => ({ computer: "", key: `run:run-${i + 1}`, row: r })) } as Batch);
  await new Promise((done) => setTimeout(done, 700));
  document.title = `${document.title}: ready`;
}
void run().catch((error: Error) => { document.title = `failed: ${String(error)} ${(error.stack ?? "").split("\n").slice(1, 3).join(" | ")}`; });
