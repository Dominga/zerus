import { beforeEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  // store/ui.ts reads the page address and storage when it loads.
  const store = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("localStorage", { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); }, removeItem: (k: string) => { store.delete(k); } });
});
vi.mock("../../bridge", () => ({
  archivePage: vi.fn(),
  cancelArchivePages: vi.fn(async () => {}),
  runAction: vi.fn(),
  inspectSession: vi.fn(),
  pendingReadMarks: vi.fn(async () => []),
  attentionMarks: vi.fn(async () => ({ marks: {}, replies: {} })),
  updateAttentionMarks: vi.fn(async () => {}),
  queueReadMarks: vi.fn(async () => []),
  sessionReference: vi.fn(),
  copyCommand: vi.fn(),
  copyText: vi.fn(),
  setNotifications: vi.fn(),
}));

import * as bridge from "../../bridge";
import { applyBatch } from "../../store/catalog";
import { archiveCount, archiveTip } from "../../store/archive";
import { stillListed } from "../../store/actions";
import { matchesFilter, selected, setMachineFilter, setSearch, setSelectedKey, setStateFilter } from "../../store/ui";
import { listRows } from "./model";
import { snapshotOf } from "./actions-ui";
import { emptyBatch, type ArchivePage, type ViewRow } from "../../types";

const mocked = vi.mocked(bridge);
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
const uuid = (n: number) => `10000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const archived = (n: number, at: number): ViewRow => ({ name: `codex/example/a${n}`, agent: "codex", state: "archived", status: "ended", archive_id: uuid(n), archived_at: at, run_id: `run-a${n}`, conversation_id: `conv-a${n}` });
const live: ViewRow = { name: "codex/example/live", agent: "codex", state: "running", status: "ready", machine: "workstation", run_id: "run-live", conversation_id: "conv-live" };

describe("the Archive view in the app", () => {
  const pending: ((p: ArchivePage) => void)[] = [];
  beforeEach(() => {
    vi.useFakeTimers();
    mocked.archivePage.mockImplementation(() => new Promise((resolve) => { pending.push(resolve); }));
  });

  it("pages only under the Archive filter, lists archived cards newest first, and finds them for the selection and the actions", async () => {
    applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key: "run:run-live", row: live }] });
    expect(mocked.archivePage).not.toHaveBeenCalled();
    expect(listRows().map((r) => r.type)).toEqual(["header", "card"]);
    expect(archiveCount()).toBe("…");

    setStateFilter("archived");
    expect(mocked.archivePage).toHaveBeenCalledWith("", 0, 253402300800, "", "", expect.any(Number));
    expect(listRows()).toEqual([]);
    pending.shift()!({ rows: [archived(1, 100), archived(2, 200)], next_cursor: "next", previous_cursor: null, total: 3, position: 0 });
    await flush();

    const rows = listRows();
    expect(rows.map((r) => r.type === "card" && [r.session.name, r.session.machine])).toEqual([["codex/example/a2", "workstation"], ["codex/example/a1", "workstation"]]);
    expect(archiveCount()).toBe("2+");
    expect(archiveTip()).toBe("Archive / 2 loaded; more can be loaded in Archive");
    expect(mocked.cancelArchivePages).toHaveBeenCalled();
    // The app numbers its requests from its start time, so a reload never reuses a cancelled number.
    expect(mocked.archivePage.mock.calls[0][5]).toBeGreaterThan(1_700_000_000_000);
    const key = `\narchive:${uuid(2)}`;
    expect(rows[0].id).toBe(key);
    if (rows[0].type === "card") expect(matchesFilter(rows[0].session, "archived")).toBe(true);

    setSelectedKey(key);
    expect(selected()?.archiveId).toBe(uuid(2));
    expect(stillListed(snapshotOf(selected()!))?.key).toBe(key);

    // The search is the archive query: the list is not filtered by text, the pages reload after 300 ms.
    setSearch("a1");
    expect(listRows()).toEqual([]);
    vi.advanceTimersByTime(300);
    expect(mocked.archivePage).toHaveBeenLastCalledWith("", 0, 253402300800, "", "a1", expect.any(Number));
    setSearch("");
    vi.advanceTimersByTime(300);
    pending.splice(0).forEach((answer) => answer({ rows: [archived(1, 100)], next_cursor: "", previous_cursor: null, total: 1, position: 0 }));
    await flush();
    expect(listRows()).toHaveLength(1);

    // A machine outside the filter pages nothing.
    const calls = mocked.archivePage.mock.calls.length;
    setMachineFilter(["build-box"]);
    expect(listRows()).toEqual([]);
    expect(mocked.archivePage.mock.calls.length).toBe(calls);
    expect(archiveCount()).toBe("0");
    setMachineFilter([]);

    setStateFilter("all");
    expect(listRows().map((r) => r.type)).toEqual(["header", "card"]);
  });
});
