// Task 242: the window's hotkeys pressed in a browser over the real session
// list and the shortcut router: Alt+Down and Alt+Up step the selection,
// Alt+Shift+Down jumps to the next session that needs attention (opening its
// collapsed project), Ctrl+F focuses the search, F5 is taken (no reload).
// Synthetic: no CLI; the only IPC answered is the settings read and Refresh.
//   http://127.0.0.1:<port>/scripts/hotkeys-smoke.html?skin=whitewh-dark
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
const calls: string[] = [];
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string) => {
    calls.push(command);
    if (command === "settings_get") return { values: {}, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    if (command === "fleet_refresh") return 0;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import { render } from "solid-js/web";
import { SessionList } from "../ui/sessions/SessionList";
import { applyBatch } from "../store/catalog";
import "../store/session";
import { installShortcutRouter } from "../store/shortcuts";
import { installWindowCommands } from "../ui/sessions/windowCommands";
import { selectedKey } from "../store/ui";
import { applySkin, skin } from "../skin/runtime";
import { emptyBatch, type Batch, type ViewRow } from "../types";

const t0 = 1793050800;
const row = (n: number, title: string, status: string, extra: Partial<ViewRow> = {}): ViewRow => ({ name: `codex/orbit/session-${n}`, title, agent: n % 2 ? "claude" : "codex", cmd: "codex",
  machine: "workstation", project: "orbit", folder: `/work/orbit/${n}`, cwd: `/work/orbit/${n}`, branch: "main", state: "running", tracked: true, run_id: `run-${n}`,
  conversation_id: `conversation-${n}`, created: t0 + n, status, activity: status === "working" ? "busy" : "idle", updated_at: t0 + 100 + n, summary: `Session ${n} summary`, ...extra } as ViewRow);

const press = (key: string, init: KeyboardEventInit = {}) => {
  const target = document.activeElement ?? document.body;
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event.defaultPrevented;
};
const tick = () => new Promise((done) => setTimeout(done, 120));

async function run() {
  await document.fonts.ready;
  applySkin(skin());
  installShortcutRouter();
  installWindowCommands();
  render(() => (
    <div class="app" style={{ height: "100vh" }}>
      <main class="page"><div class="sessions-page"><SessionList /><div style={{ flex: "1", display: "grid", "place-items": "center", color: "var(--z-color-muted)" }}>Conversation</div></div></main>
    </div>
  ), document.getElementById("root")!);
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [
    row(1, "Saved cards API", "working"), row(2, "Card form UI", "ready"), row(3, "Contract tests", "attention", { attention: "approval", phase: "approval" }),
    row(4, "Release notes", "ready"), row(5, "Load test", "ready"),
  ].map((r, i) => ({ computer: "", key: `run:run-${i + 1}`, row: r })) } as Batch);
  await new Promise((done) => setTimeout(done, 700));
  document.querySelector<HTMLElement>(".sp-scroll")?.focus();
  const log: { keys: string; selected?: string; consumed: boolean; focus?: string }[] = [];
  const step = async (keys: string, key: string, init: KeyboardEventInit = {}) => {
    const consumed = press(key, init);
    await tick();
    log.push({ keys, selected: selectedKey()?.split(":").pop(), consumed, focus: (document.activeElement as HTMLElement | null)?.getAttribute("type") ?? document.activeElement?.className });
  };
  await step("Alt+Down", "ArrowDown", { altKey: true, code: "ArrowDown" });
  await step("Alt+Down", "ArrowDown", { altKey: true, code: "ArrowDown" });
  await step("Alt+Up", "ArrowUp", { altKey: true, code: "ArrowUp" });
  await step("Alt+Shift+Down", "ArrowDown", { altKey: true, shiftKey: true, code: "ArrowDown" });
  await step("F5", "F5", { code: "F5" });
  await step("Ctrl+F", "f", { ctrlKey: true, code: "KeyF" });
  (window as unknown as { __hotkeys: unknown }).__hotkeys = { log, refreshed: calls.includes("fleet_refresh") };
  await new Promise((done) => setTimeout(done, 300));
  document.title = `${document.title}: ready`;
}
void run().catch((error: Error) => { document.title = `failed: ${String(error)} ${(error.stack ?? "").split("\n").slice(1, 3).join(" | ")}`; });
