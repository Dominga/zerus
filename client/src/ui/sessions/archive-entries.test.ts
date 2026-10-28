import { describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  // The scope rule lives in store/archive.ts, which loads store/ui.ts (the page address and storage).
  const store = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("localStorage", { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); }, removeItem: (k: string) => { store.delete(k); } });
});
vi.mock("../../bridge", () => ({
  archivePage: vi.fn(() => new Promise(() => {})),
  cancelArchivePages: vi.fn(async () => {}),
  pendingReadMarks: vi.fn(async () => []),
  attentionMarks: vi.fn(async () => ({ marks: {}, replies: {} })),
  updateAttentionMarks: vi.fn(async () => {}),
  queueReadMarks: vi.fn(async () => []),
}));

import { CLEAR_ARCHIVE_LABEL, clearArchiveEntry, clearArchiveRows } from "./archive-entries";

// Qt's Clear archive (SessionSelection.cpp 171-181 and 226-235).

const row = (name: string, machine: string, project: string, archivedAt: number, state = "archived") => ({ name, machine, project, archivedAt, state });
const rows = [
  row("a", "box", "Inbox", 100),
  row("b", "box", "Docs", 200),
  row("c", "laptop", "Inbox", 300),
  row("d", "box", "Inbox", 50_000),
  row("live", "box", "Inbox", 0, "running"),
];
const all = { machines: [], projects: [] };

describe("clearArchiveRows", () => {
  it("takes the loaded archived rows in the machine and project filters and the date range", () => {
    const everything = () => true;
    expect(clearArchiveRows(rows, { ...all, includes: everything }).map((r) => r.name)).toEqual(["a", "b", "c", "d"]);
    expect(clearArchiveRows(rows, { machines: ["box"], projects: [], includes: everything }).map((r) => r.name)).toEqual(["a", "b", "d"]);
    expect(clearArchiveRows(rows, { machines: [], projects: ["Inbox"], includes: everything }).map((r) => r.name)).toEqual(["a", "c", "d"]);
    expect(clearArchiveRows(rows, { machines: ["box"], projects: ["Inbox"], includes: (at) => at < 1000 }).map((r) => r.name)).toEqual(["a"]);
  });
});

describe("clearArchiveEntry", () => {
  const run = vi.fn();
  it("is Qt's paged label, dangerous, and runs the given batch", () => {
    const e = clearArchiveEntry(rows, all, () => true, false, run);
    expect(CLEAR_ARCHIVE_LABEL).toBe("Remove loaded archived sessions…");
    expect(e).toMatchObject({ label: CLEAR_ARCHIVE_LABEL, icon: "trash", danger: true, disabled: false });
    e.action?.();
    expect(run).toHaveBeenCalledTimes(1);
  });
  it("is enabled only when some archived row in the machine and project filters can be forgotten and no batch runs", () => {
    expect(clearArchiveEntry(rows, all, () => true, true, run).disabled).toBe(true);
    expect(clearArchiveEntry(rows, all, () => false, false, run).disabled).toBe(true);
    expect(clearArchiveEntry(rows, all, (s) => s.name === "c", false, run).disabled).toBe(false);
    expect(clearArchiveEntry(rows, { machines: ["box"], projects: [] }, (s) => s.name === "c", false, run).disabled).toBe(true);
    expect(clearArchiveEntry(rows, { machines: [], projects: ["Docs"] }, (s) => s.name === "b", false, run).disabled).toBe(false);
    // A live row never counts, and the date range does not limit the count (Qt 171-176).
    expect(clearArchiveEntry(rows, all, (s) => s.name === "live", false, run).disabled).toBe(true);
    expect(clearArchiveEntry([], all, () => true, false, run).disabled).toBe(true);
  });
});
