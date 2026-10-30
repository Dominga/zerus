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
  it("keeps a known same-selection page on read failure, with no automatic retry", async () => {
    const read = vi.fn().mockResolvedValueOnce(raw()).mockRejectedValue(new Error("offline"));
    const s = createTaskDetails(read); s.select("p", "one"); await tick(); s.refresh(); await tick();
    expect(s.error()).toContain("offline"); expect(s.data()?.uuid).toBe("one"); expect(read).toHaveBeenCalledTimes(2);
    s.select("p", "two"); await tick(); expect(s.data()).toBeUndefined();
  });
  it("discards a read after close and refuses wrong identities and legacy journals", async () => {
    const first = deferred(), read = vi.fn().mockReturnValue(first.promise), s = createTaskDetails(read);
    s.select("p", "one"); s.select(); first.resolve(raw()); await tick(); expect(s.data()).toBeUndefined();
    expect(() => taskDetail(raw(), "other", "one")).toThrow("selected task");
    expect(() => taskDetail(raw("p", "one", { history_schema: undefined }), "p", "one")).toThrow("Update hgs");
    expect(() => taskDetail(raw("p", "one", { log: Array.from({ length: 81 }, () => ({})) }), "p", "one")).toThrow("Update hgs");
  });
});
