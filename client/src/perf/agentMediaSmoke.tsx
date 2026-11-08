// Task 248: the media an agent names, beside Qt's agentMediaShowsPreviewsUnderTheMessage
// (tray/tests/test_activityview.cpp): a reply naming a picture, a video and a
// text file, a tool group that saved a screenshot, and Qt's notes for a
// peer's video, a missing file and a file too large to preview. `?viewer=1`
// clicks the first tile: the MediaViewer dialog. Synthetic: the files are
// drawn on a canvas; session_media answers from a table here; no CLI, session
// or owner data. Served by Vite:
//   http://127.0.0.1:<port>/scripts/agent-media-smoke.html?skin=whitewh-dark[&viewer=1]
// and captured over CDP (scripts/plan-smoke.mjs <out> <chrome> agent-media).
const pictures = new Map<string, string>();
function drawn(token: string, colours: [string, string], label: string) {
  const canvas = document.createElement("canvas");
  canvas.width = 960; canvas.height = 600;
  const g = canvas.getContext("2d")!;
  const gradient = g.createLinearGradient(0, 0, 960, 600);
  gradient.addColorStop(0, colours[0]); gradient.addColorStop(1, colours[1]);
  g.fillStyle = gradient; g.fillRect(0, 0, 960, 600);
  g.fillStyle = "rgba(255,255,255,.88)"; g.font = "600 64px sans-serif"; g.fillText(label, 60, 320);
  pictures.set(token, canvas.toDataURL("image/png"));
}
const HOME = "a".repeat(32), CHART = "b".repeat(32);
const answers: Record<string, Record<string, unknown>> = {
  "/work/orbit/shots/home.png": { state: "ready", token: HOME, kind: "image", size: 182_000, name: "home.png" },
  "/work/orbit/out/chart.png": { state: "ready", token: CHART, kind: "image", size: 51_000, name: "chart.png" },
  "/work/orbit/demo/checkout.mp4": { state: "click", kind: "video", name: "checkout.mp4" },
  "/work/orbit/shots/gone.png": { state: "error", code: "missing", kind: "image", name: "gone.png" },
  "/work/orbit/shots/full.png": { state: "error", code: "too_large", kind: "image", name: "full.png" },
};
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  convertFileSrc: (path: string) => pictures.get(path.replace(/^media\//, "")) ?? "",
  invoke: async (command: string, args: Record<string, unknown>) => {
    if (command === "settings_get") return { values: {}, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    if (command === "session_media") {
      const href = String(args.href).replace(/^file:\/\//, "");
      return { request: args.request, code: "", detail: "", token: "", size: 0, ...(answers[href] ?? { state: "error", code: "not_media", kind: "none", name: href }) };
    }
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

// The app's style order (main.tsx): base.css declares the cascade layers first.
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { ActivityView } from "../ui/sessions/Activity";
import { loadMarkdown } from "../ui/sessions/markdown";
import { applyBatch } from "../store/catalog";
import type { ActivitySource } from "../store/activitySource";
import { emptyBatch, type ActivityEvent, type Batch, type ViewRow } from "../types";
import { applySkin, skin } from "../skin/runtime";

const at = 1793050800;
const event = (seq: number, type: string, detail: string, tool?: string) => ({ seq, type, detail, tool, at: at + seq * 20 }) as unknown as ActivityEvent;
const events = [
  event(1, "UserPromptSubmit", "Take screenshots of the new checkout and record the flow."),
  event(2, "PreToolUse", "node scripts/shots.mjs", "Bash"),
  event(3, "PostToolUse", "Saved /work/orbit/out/chart.png (1440x900)", "Bash"),
  event(4, "Stop", "Done: ![home](file:///work/orbit/shots/home.png)\n\nRecorded [the flow](file:///work/orbit/demo/checkout.mp4) and wrote [notes](file:///work/orbit/notes.txt). "
    + "The old [screenshot](/work/orbit/shots/gone.png) is gone; [the full page](/work/orbit/shots/full.png) is too large to preview."),
];
const row = { name: "codex/orbit/checkout", title: "Checkout shots", agent: "codex", cmd: "codex", machine: "workstation", project: "orbit", folder: "/work/orbit",
  cwd: "/work/orbit", branch: "main", state: "running", tracked: true, run_id: "run-1", conversation_id: "conversation-1", created: at, status: "idle",
  activity: "idle", updated_at: at + 100 } as ViewRow;
const [target] = createSignal({ id: "\nrun:run-1" });
const source = {
  target, events: () => events, summary: () => ({ conversation_id: "conversation-1" }), loaded: () => true,
  loadingOlder: () => false, olderDone: () => true, ended: () => false, error: () => undefined, readOnly: true, loadOlder: () => {},
} as unknown as ActivitySource;

async function run() {
  await document.fonts.ready;
  drawn(HOME, ["#2f66c9", "#8e9bff"], "Checkout: home");
  drawn(CHART, ["#137f95", "#b9d36b"], "Conversion chart");
  await loadMarkdown(); applySkin(skin());
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key: "run:run-1", row }] } as Batch);
  const root = document.getElementById("root")!;
  root.style.cssText = "display:flex;width:760px;height:1000px;--col:100%;--content-scale:1";
  render(() => <ActivityView source={source} />, root);
  const deadline = performance.now() + 10000;
  while (root.querySelector<HTMLElement>(".act-scroll > .act-col")?.style.visibility !== "visible") {
    if (performance.now() > deadline) throw Error("Activity was never placed");
    await new Promise((done) => setTimeout(done, 20));
  }
  root.querySelector<HTMLElement>(".act-scroll")?.scrollTo({ top: 0 });
  await new Promise((done) => setTimeout(done, 500));
  if (new URLSearchParams(location.search).get("viewer") === "1") {
    root.querySelector<HTMLButtonElement>(".act-msg.k-agent .media-tile")?.click();
    await new Promise((done) => setTimeout(done, 500));
  }
  document.title = `${document.title}: ready`;
}
void run().catch((error: Error) => { document.title = `failed: ${String(error)} ${(error.stack ?? "").split("\n").slice(1, 3).join(" | ")}`; });
