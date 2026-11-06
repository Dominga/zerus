import { describe, expect, it, vi } from "vitest";
import { createTaskDetails, taskDetail } from "./taskDetails";

const raw = (project = "p", uuid = "one", extra = {}) => ({ uuid, project, title: uuid, stage: "todo", text: "Canonical",
  history_schema: 1, history_next: null, log_total: 1, log: [{ kind: "comment", text: "Note", actor: { kind: "person" }, at: 1 }], ...extra });
const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
const deferred = () => { let resolve!: (v: unknown) => void; const promise = new Promise<unknown>((r) => { resolve = r; }); return { promise, resolve }; };

describe("bounded task inspector", () => {
  it("serializes selection changes and discards the previous task's response", async () => {
    const first = deferred(), last = deferred(); const read = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(last.promise);
    const s = createTaskDetails(read); s.select("p", "one"); s.select("p", "two"); s.select("other", "three");
    expect(read.mock.calls).toEqual([["p", "one", null]]);
    first.resolve(raw()); await tick(); expect(s.data()).toBeUndefined();
    expect(read.mock.calls).toEqual([["p", "one", null], ["other", "three", null]]);
    last.resolve(raw("other", "three")); await tick(); expect(s.data()?.uuid).toBe("three");
    s.select(); expect(s.data()).toBeUndefined(); expect(s.loading()).toBe(false);
  });
  it("publishes a same-selection reply while board refreshes coalesce behind it", async () => {
    const first = deferred(), second = deferred(); const read = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const s = createTaskDetails(read); s.select("p", "one"); s.refresh(); s.refresh(); s.refresh();
    first.resolve(raw()); await tick(); expect(s.data()?.text).toBe("Canonical"); expect(read).toHaveBeenCalledTimes(2);
    second.resolve(raw("p", "one", { text: "Updated" })); await tick(); expect(s.data()?.text).toBe("Updated");
    expect(s.loading()).toBe(false); expect(read).toHaveBeenCalledTimes(2);
  });
  it("replaces pages and preserves an older cursor across incoming board updates", async () => {
    const read = vi.fn().mockResolvedValueOnce(raw("p", "one", { history_next: "older" }))
      .mockResolvedValue(raw("p", "one", { log: [{ kind: "created", text: "Earlier" }], log_total: 100 }));
    const s = createTaskDetails(read); s.select("p", "one"); await tick(); s.older(); await tick();
    expect(s.before()).toBe("older"); expect(s.data()?.log.map((e) => e.text)).toEqual(["Earlier"]);
    s.refresh(); await tick(); expect(read.mock.calls.at(-1)).toEqual(["p", "one", "older"]);
    s.latest(); await tick(); expect(read.mock.calls.at(-1)).toEqual(["p", "one", null]);
    expect(s.data()?.log).toHaveLength(1);
  });
  it("keeps unchanged entries and files by identity on a refresh, so their nodes stay (zerus-agv9)", async () => {
    const file = (name: string) => ({ name, path: `/cache/${name}`, bytes: 2048, mime: "text/plain", thumbnail: false });
    const note = (text: string, at: number, state = "note") => ({ kind: "comment", text, at, actor: { kind: "person" }, delivery: { mode: "", state, error: "" } });
    const page = (log: unknown[], attachments = [file("spec.txt")]) => raw("p", "one", { attachments, log, log_total: log.length });
    const read = vi.fn()
      .mockResolvedValueOnce(page([note("First", 1), note("Second", 2)]))
      .mockResolvedValueOnce(page([note("First", 1), note("Second", 2)]))
      .mockResolvedValueOnce(page([note("First", 1), note("Second", 2, "queued"), note("Third", 3)], [file("spec.txt"), file("more.txt")]))
      .mockResolvedValueOnce(page([note("Same", 4), note("Same", 4)]));
    const s = createTaskDetails(read); s.select("p", "one"); await tick();
    const first = s.data()!;
    s.refresh(); await tick();
    const same = s.data()!;
    expect(same).not.toBe(first);
    expect(same.log[0]).toBe(first.log[0]); expect(same.log[1]).toBe(first.log[1]);
    expect(same.files).toBe(first.files);
    s.refresh(); await tick();
    const changed = s.data()!;
    expect(changed.log[0]).toBe(first.log[0]);
    expect(changed.log[1]).not.toBe(first.log[1]); expect(changed.log[1].delivery.state).toBe("queued");
    expect(changed.log.map((e) => e.text)).toEqual(["First", "Second", "Third"]);
    expect(changed.files).not.toBe(first.files); expect(changed.files[0]).toBe(first.files[0]);
    expect(changed.files.map((f) => f.name)).toEqual(["spec.txt", "more.txt"]);
    // Two identical records stay two rows.
    s.refresh(); await tick();
    expect(s.data()!.log).toHaveLength(2); expect(s.data()!.log[0]).not.toBe(s.data()!.log[1]);
    // Another selection starts from its own reply.
    s.select("p", "two"); expect(s.data()).toBeUndefined();
  });
  it("keeps a known same-selection page on read failure, with no automatic retry", async () => {
    const read = vi.fn().mockResolvedValueOnce(raw()).mockRejectedValue(new Error("offline"));
    const s = createTaskDetails(read); s.select("p", "one"); await tick(); s.refresh(); await tick();
    expect(s.error()).toContain("offline"); expect(s.data()?.uuid).toBe("one"); expect(read).toHaveBeenCalledTimes(2);
    s.select("p", "two"); await tick(); expect(s.data()).toBeUndefined();
  });
  it("keeps why a report did not move its task, and only when there is a reason", () => {
    const log = [{ kind: "report", text: "Late", actor: { kind: "agent", session: "codex/old" }, at: 2, stale: "other_executor" },
      { kind: "progress", text: "Half way", actor: { kind: "agent", session: "codex/new" }, at: 3, considered: 1 },
      { kind: "report", text: "Done", actor: { kind: "agent", session: "codex/new" }, at: 4, stale: "" }];
    const entries = taskDetail(raw("p", "one", { log, log_total: 3 }), "p", "one").log;
    expect(entries.map((e) => [e.kind, e.stale])).toEqual([["report", "other_executor"], ["progress", undefined], ["report", undefined]]);
    expect(entries.every((e, i) => i === 0 || !("stale" in e))).toBe(true);
  });
  it("keeps the attempt an entry's request names, and only when there is one", () => {
    const log = [{ kind: "message", text: "Please change it", at: 2, request_id: "r", delivery: { mode: "note", state: "note" }, attempt: { origin: "reopen", phase: "submitted" } },
      { kind: "message", text: "Unreadable", at: 3, attempt: null },
      { kind: "comment", text: "Plain", at: 4 }];
    const entries = taskDetail(raw("p", "one", { log, log_total: 3 }), "p", "one").log;
    expect(entries.map((e) => e.attempt)).toEqual([{ origin: "reopen", phase: "submitted" }, { origin: "", phase: "" }, undefined]);
    expect("attempt" in entries[2]).toBe(false);
  });
  it("discards a read after close and refuses wrong identities and legacy journals", async () => {
    const first = deferred(), read = vi.fn().mockReturnValue(first.promise), s = createTaskDetails(read);
    s.select("p", "one"); s.select(); first.resolve(raw()); await tick(); expect(s.data()).toBeUndefined();
    expect(() => taskDetail(raw(), "other", "one")).toThrow("selected task");
    expect(() => taskDetail(raw("p", "one", { history_schema: undefined }), "p", "one")).toThrow("Update hgs");
    expect(() => taskDetail(raw("p", "one", { log: Array.from({ length: 81 }, () => ({})) }), "p", "one")).toThrow("Update hgs");
  });
});
