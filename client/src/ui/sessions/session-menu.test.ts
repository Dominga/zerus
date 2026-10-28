import { describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  // store/ui.ts reads the page address and storage when it loads.
  const store = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("localStorage", { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); }, removeItem: (k: string) => { store.delete(k); } });
  // The list module registers Solid's delegated events when it loads; nothing renders here.
  vi.stubGlobal("window", { document: { addEventListener: () => {} } });
});
vi.mock("../../bridge", () => ({
  archivePage: vi.fn(() => new Promise(() => {})),
  cancelArchivePages: vi.fn(async () => {}),
  runAction: vi.fn(),
  inspectSession: vi.fn(async () => null),
  pendingReadMarks: vi.fn(async () => []),
  attentionMarks: vi.fn(async () => ({ marks: {}, replies: {} })),
  updateAttentionMarks: vi.fn(async () => {}),
  queueReadMarks: vi.fn(async () => []),
  sessionReference: vi.fn(),
  copyCommand: vi.fn(),
  copyText: vi.fn(),
  setNotifications: vi.fn(),
}));

import { sessionMenu } from "./SessionList";
import { archivedSession } from "../../store/archive";
import { toSession } from "../../store/catalog";
import type { MenuEntry } from "../../store/ui";
import type { ViewRow } from "../../types";

const labels = (entries: MenuEntry[]) => entries.map((e) => ("separator" in e ? "---" : "heading" in e ? `# ${e.heading}` : e.label));
const archived = (agent: string): ViewRow => ({
  name: `${agent}/example/a1`, agent, state: "archived", status: "ended", archive_id: "10000000-0000-4000-8000-000000000001",
  archived_at: 100, run_id: "run-a1", conversation_id: "conv-a1", resumable: true,
});

describe("the session menu (Qt showSessionMenu 5266-5382)", () => {
  it("an archived row has no terminal entry and no Mute or Mark group, and no doubled separators", () => {
    const expected = [
      "Compact session row", "Expand all collapsed", "---",
      "Open folder", "Open shell in session folder", "Copy restore command", "---",
      "Restore session", "Rename…", "Fork session…", "Move to project…", "---",
      "Copy session name", "Copy session reference", "Copy folder path", "---",
      "Forget archived session…",
    ];
    expect(labels(sessionMenu(archivedSession("", "Local", archived("codex"))))).toEqual(expected);
    expect(labels(sessionMenu(archivedSession("", "Local", archived("dsh"))))).toEqual(expected);
  });

  it("a live row keeps its terminal entry and the Mute and Mark group", () => {
    const live = toSession("", "run:run-live", { name: "codex/example/live", agent: "codex", state: "running", status: "ready", run_id: "run-live", conversation_id: "conv-live" });
    expect(labels(sessionMenu(live))).toEqual([
      "Compact session row", "Expand all collapsed", "---",
      "Open terminal", "Open folder", "Open shell in session folder", "Copy connect command", "---",
      "Mute", "Mark needs attention", "---",
      "Pause session", "Clear session…", "Rename…", "Fork session…", "Move to project…", "---",
      "Copy session name", "Copy session reference", "Copy folder path", "---",
      "Terminate session…",
    ]);
  });
});
