// Task 252: the session list's git chips fed by the desktop's git status
// (the real list, gitFeed and catalog; a fake git_status_request answering
// like `hgs git-status`): changes with conflicts, outgoing, incoming, in sync
// and not checked; a narrow list folds git into the More chip.
//   http://127.0.0.1:<port>/scripts/git-chips-smoke.html?skin=whitewh-dark[&width=300]
const query = new URLSearchParams(location.search);
const now = Date.now();
const answers: Record<string, Record<string, unknown>> = {
  "/work/api": { state: "ok", branch: "main", changed_files: 3, conflicts: 1, ahead: 2, behind: 0, remote_state: "verified", remote_age_seconds: 4 },
  "/work/web": { state: "ok", branch: "main", changed_files: 0, ahead: 0, behind: 5, remote_state: "verified", remote_age_seconds: 4 },
  "/work/docs": { state: "ok", branch: "main", changed_files: 0, ahead: 0, behind: 0, remote_state: "verified", remote_age_seconds: 4 },
  "/work/tools": { state: "ok", branch: "main", changed_files: 7, ahead: 1, behind: 0, remote_state: "no_upstream" },
  "/work/plain": { state: "not_repo" },
};
const asked: string[] = [];
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string, args: { paths?: string[]; computer?: string }) => {
    if (command === "settings_get") return { values: {}, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    if (command === "git_status_request") {
      asked.push(...(args.paths ?? []));
      return (args.paths ?? []).filter((path) => answers[path]).map((path) => ({ computer: args.computer ?? "", path, status: { ...answers[path], received_at_ms: now } }));
    }
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
import { applySkin, skin } from "../skin/runtime";
import { setSetting } from "../store/settings";
import { emptyBatch, type Batch, type ViewRow } from "../types";

const t0 = 1793050800;
const row = (n: number, title: string, path: string, status: string): ViewRow => ({ name: `codex/orbit/session-${n}`, title, agent: n % 2 ? "claude" : "codex", cmd: "codex",
  machine: "workstation", project: "orbit", folder: path.split("/").pop(), cwd: path, cwd_canonical: path, git_root: path === "/work/plain" ? "" : path, branch: "main",
  state: "running", tracked: true, run_id: `run-${n}`, conversation_id: `conversation-${n}`, created: t0 + n, status, activity: status === "working" ? "busy" : "idle",
  updated_at: t0 + 100 + n, summary: `Session ${n} summary` } as ViewRow);

async function run() {
  await document.fonts.ready;
  applySkin(skin());
  const width = query.get("width");
  if (width) await setSetting("workspace/sessionsWidth", Number(width));
  render(() => (
    <div style={{ height: "100vh", display: "flex" }}>
      <main class="page" style={{ flex: "1", "min-width": "0" }}><div class="sessions-page"><SessionList /><div style={{ flex: "1" }} /></div></main>
    </div>
  ), document.getElementById("root")!);
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [
    row(1, "Saved cards API", "/work/api", "working"), row(2, "Card form UI", "/work/web", "ready"), row(3, "Docs pass", "/work/docs", "ready"),
    row(4, "Build tools", "/work/tools", "ready"), row(5, "Notes", "/work/plain", "ready"), row(6, "Scratch", "/work/scratch", "ready"),
  ].map((r, i) => ({ computer: "", key: `run:run-${i + 1}`, row: r })) } as Batch);
  await new Promise((done) => setTimeout(done, 1200));
  (window as unknown as { __git: unknown }).__git = {
    asked: [...new Set(asked)],
    chips: [...document.querySelectorAll(".scard")].map((card) => [...card.querySelectorAll<HTMLElement>("[data-chip]")].map((chip) => `${chip.dataset.chip}:${chip.textContent ?? ""}`).join(" ")),
  };
  document.title = `${document.title}: ready`;
}
void run().catch((error: Error) => { document.title = `failed: ${String(error)} ${(error.stack ?? "").split("\n").slice(1, 3).join(" | ")}`; });
