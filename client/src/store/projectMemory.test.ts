import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createProjectMemory, movedText, sizeText, statusText } from "./projectMemory";
import type { MemoryCall, MemoryOutcome, MemoryView } from "../bridge";

// Task 126: Qt's ProjectMemoryPanel over client/core's guarded `hgs project config` calls.

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const view = (patch: Partial<MemoryView> = {}): MemoryView => ({
  enabled: false, store: "/state/lore/projects/p-1", default_store: "/state/lore/projects/p-1", default: true, revision: 0,
  moved_from: [], exists: false, records: 0, files: 0, bytes: 0, index: "none", ...patch,
});
const done = (patch: Partial<MemoryView> = {}): MemoryOutcome => ({ outcome: "done", view: view(patch) });

function fixture(answers: (call: MemoryCall) => MemoryOutcome, options: { picked?: string | null; confirm?: boolean } = {}) {
  const calls: MemoryCall[] = [];
  const asked: [string, string, number][] = [];
  const memory = createRoot(() => createProjectMemory({
    call: async (call) => { calls.push(call); return answers(call); },
    pickFolder: async () => (options.picked === undefined ? "/srv/memory" : options.picked),
    confirmMove: async (from, to, records) => { asked.push([from, to, records]); return options.confirm ?? true; },
  }));
  return { memory, calls, asked };
}

describe("project memory", () => {
  it("reads the project's memory and turns it on at the revision it read", async () => {
    let state = view();
    const { memory, calls } = fixture((call) => {
      if (call.op.op === "enable") state = { ...state, enabled: call.op.enabled, exists: true, revision: state.revision + 1 };
      return { outcome: "done", view: state };
    });
    memory.setProject("p-1");
    await tick();
    expect(calls[0]).toEqual({ project: "p-1", op: { op: "read" } });
    expect(memory.view()?.enabled).toBe(false);
    expect(await memory.setEnabled(true)).toBe(true);
    expect(calls[1].op).toEqual({ op: "enable", enabled: true, expected_revision: 0 });
    expect(memory.view()?.enabled).toBe(true);
    // Asking for what it already is sends nothing; the same project reads nothing again.
    expect(await memory.setEnabled(true)).toBe(false);
    memory.setProject("p-1");
    expect(calls).toHaveLength(2);
  });

  it("Browse repoints an empty store and moves one with records after the owner confirms", async () => {
    const empty = fixture(() => done({ revision: 3 }));
    empty.memory.setProject("p-1");
    await tick();
    expect(await empty.memory.browse()).toBe(true);
    expect(empty.calls[1].op).toEqual({ op: "store", store: "/srv/memory", expected_revision: 3 });
    expect(empty.asked).toEqual([]);

    const full = fixture((call) => (call.op.op === "move"
      ? { outcome: "done", view: view({ store: "/srv/memory", default: false, records: 4, exists: true, revision: 4, moved_from: ["/state/lore/projects/p-1"] }),
          moved: { from: "/state/lore/projects/p-1", to: "/srv/memory", files: 3, bytes: 2048, old_kept: true } }
      : done({ records: 4, exists: true, revision: 3 })));
    full.memory.setProject("p-1");
    await tick();
    expect(await full.memory.browse()).toBe(true);
    expect(full.asked).toEqual([["/state/lore/projects/p-1", "/srv/memory", 4]]);
    expect(full.calls[1].op).toEqual({ op: "move", to: "/srv/memory", expected_revision: 3 });
    expect(full.memory.view()?.store).toBe("/srv/memory");
    expect(full.memory.notice()).toContain("The old store is kept at /state/lore/projects/p-1");
  });

  it("Cancel moves nothing, a cancelled dialog sends nothing, and an uncertain write reads again", async () => {
    const cancelled = fixture(() => done({ records: 2, exists: true }), { confirm: false });
    cancelled.memory.setProject("p");
    await tick();
    expect(await cancelled.memory.moveTo("/elsewhere")).toBe(false);
    expect(cancelled.calls).toHaveLength(1);
    const closed = fixture(() => done(), { picked: null });
    closed.memory.setProject("p");
    await tick();
    expect(await closed.memory.browse()).toBe(false);
    expect(closed.calls).toHaveLength(1);
    let written = false;
    const lost = fixture((call) => {
      if (call.op.op !== "read") { written = true; return { outcome: "uncertain", detail: { source: "timed out" }, request_id: "r" }; }
      return written ? done({ enabled: true, revision: 1 }) : done();
    });
    lost.memory.setProject("p");
    await tick();
    expect(await lost.memory.setEnabled(true)).toBe(false);
    await tick();
    expect(lost.calls.map((call) => call.op.op)).toEqual(["read", "enable", "read"]);
    expect(lost.memory.view()?.enabled).toBe(true);
    expect(lost.memory.error()).toEqual({ source: "timed out" });
  });

  it("an older hgs leaves the panel read-only with its reason", async () => {
    const { memory } = fixture(() => ({ outcome: "unsupported", detail: { source: "Update this computer's hgs to set project memory." } }));
    memory.setProject("p");
    await tick();
    expect(memory.failed()).toBe(true);
    expect(memory.editable()).toBe(false);
    expect(await memory.setEnabled(true)).toBe(false);
  });

  it("names records, size and the index state", () => {
    expect(statusText(view())).toContain("No store yet");
    expect(statusText(view({ exists: true, records: 12, bytes: 3 * 1024 * 1024, index: "stale" }))).toBe("12 records, 3.0 MiB, index out of date");
    expect(sizeText(512)).toBe("512 B");
    expect(movedText({ from: "/a", to: "/b", files: 2, bytes: 2048, old_kept: true })).toBe("Moved 2 files (2.0 KiB) and checked them. The old store is kept at /a.");
  });

  // Task #397: a search over the project and its team sub-stores, each hit
  // named with its store, and the model notice the owner can dismiss.
  it("searches every store and keeps the notes until they are dismissed", async () => {
    const { memory, calls } = fixture((call) => call.op.op === "find"
      ? { outcome: "found", store: "/state/lore/projects/p-1", query: call.op.query, notes: ["fetching the model (12%)"],
          hits: [
            { id: "p1", kind: "decision", title: "Warm the cache", store: "project", via: "text", score: 0.8 },
            { id: "t1", kind: "fault", title: "Cold starts", store: "team Core", via: "text", score: 0.4 },
          ] }
      : done({ enabled: true, exists: true, revision: 2, index: "built" }));
    memory.setProject("p-1");
    await tick();
    await memory.search("  cache warm  ");
    expect(calls[1]).toEqual({ project: "p-1", op: { op: "find", query: "cache warm", limit: 10 } });
    expect(memory.found()?.hits.map((hit) => [hit.id, hit.store])).toEqual([["p1", "project"], ["t1", "team Core"]]);
    expect(memory.openNotes()).toEqual(["fetching the model (12%)"]);
    memory.dismissNotes();
    expect(memory.openNotes()).toEqual([]);
    // A refused search says why and shows no hits.
    const refused = fixture(() => ({ outcome: "failed", detail: { source: "no lore binary" } }));
    refused.memory.setProject("p-1");
    await tick();
    await refused.memory.search("cache");
    expect(refused.memory.found()).toBeUndefined();
    expect(refused.memory.searchError()).toEqual({ source: "no lore binary" });
  });
});
