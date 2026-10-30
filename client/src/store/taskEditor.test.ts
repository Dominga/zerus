import { describe, expect, it, vi } from "vitest";
import { createTaskEditor, type TaskEditorPort, type TaskEditorView, type TaskEditorTarget } from "./taskEditor";
vi.mock("./actions", () => ({ notify: vi.fn() }));
vi.mock("./exitFlush", () => ({ exitFlush: { register: vi.fn() } }));
vi.mock("../bridge", () => ({}));
const target = (task: string): TaskEditorTarget => ({ project: "example", task, mode: "note" });
const view = (task: string, extra = {}): TaskEditorView => ({ ...target(task), handle: task === "one" ? 1 : 2, rev: 0, text: "Imported", position: 4, anchor: 2, files: [], blocked: false, recovery: false, error: "", saved: false, ...extra });
const deferred = <T,>() => { let resolve!: (v: T) => void; let reject!: (e: Error) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const tick = async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); };
const port = (extra: Partial<TaskEditorPort> = {}): TaskEditorPort => ({ open: vi.fn(async (t) => view(t.task)), edit: vi.fn(async (input) => ({ ...view("one"), ...input })), pick: vi.fn(async () => view("one")), remove: vi.fn(async () => view("one")), submit: vi.fn(async () => view("one", { saved: true, text: "" })), ...extra });
describe("selected task draft editor", () => {
  it("serializes opens and discards old responses while selecting another task", async () => {
    const first = deferred<TaskEditorView>(), p = port({ open: vi.fn().mockReturnValueOnce(first.promise).mockResolvedValueOnce(view("two")) });
    const s = createTaskEditor(p); s.follow(target("one")); await tick(); s.follow(target("two")); await tick(); expect(p.open).toHaveBeenCalledTimes(1);
    first.resolve(view("one")); await tick(); expect(s.view()?.task).toBe("two"); expect(p.open).toHaveBeenCalledTimes(2);
  });
  it("keeps newer typing when a save completes and flushes it before submission", async () => {
    const first = deferred<TaskEditorView>(), p = port({ edit: vi.fn(async (input) => ({ ...view("one"), ...input })).mockReturnValueOnce(first.promise) });
    const s = createTaskEditor(p); s.follow(target("one")); await tick(); s.edit("First", 5, 5); const flushing = s.flush(); s.edit("Newer", 5, 3);
    first.resolve(view("one", { text: "First", rev: 1 })); await flushing; expect(s.view()?.text).toBe("Newer"); expect(p.edit).toHaveBeenCalledTimes(2);
    s.edit("Final", 5, 5); await s.submit(); expect(p.submit).toHaveBeenCalledExactlyOnceWith(1, 3, false); expect(p.edit).toHaveBeenCalledTimes(3); expect(s.view()?.text).toBe("");
  });
  it("saves the old key before switching and retains its imported cursor", async () => {
    const p = port(), s = createTaskEditor(p); s.follow(target("one")); await tick(); expect(s.view()?.position).toBe(4); expect(s.view()?.anchor).toBe(2);
    s.edit("Old draft", 7, 3); s.follow(target("two")); await tick(); expect(p.edit).toHaveBeenCalledWith({ handle: 1, rev: 1, text: "Old draft", position: 7, anchor: 3 }); expect(s.view()?.task).toBe("two");
  });
  it("keeps unsaved text after storage failure and does not submit it", async () => {
    const p = port({ edit: vi.fn().mockRejectedValue(new Error("disk full")) }), s = createTaskEditor(p); s.follow(target("one")); await tick(); s.edit("Kept", 4, 4); await s.submit();
    expect(p.submit).not.toHaveBeenCalled(); expect(s.view()?.text).toBe("Kept"); expect(s.error()).toContain("disk full");
  });
  it("never replays an uncertain submit; recovery is a separate explicit call", async () => {
    const p = port({ submit: vi.fn().mockResolvedValueOnce(view("one", { recovery: true, error: "unknown" })).mockResolvedValueOnce(view("one", { saved: true, text: "" })) }), s = createTaskEditor(p);
    s.follow(target("one")); await tick(); await s.submit(); await tick(); expect(p.submit).toHaveBeenCalledTimes(1); expect(s.view()?.recovery).toBe(true);
    await s.submit(true); expect(p.submit).toHaveBeenNthCalledWith(2, 1, 0, true);
  });
  it("checkpoints an attachment save failure before switching away", async () => {
    const file = { path: "cache/example/file.txt", name: "file.txt", mime: "text/plain", bytes: 4, thumbnail: false, reference: "[File #1]" };
    const p = port({ pick: vi.fn().mockResolvedValue(view("one", { files: [file], unsaved: true, error: "disk full", rev: 1 })) }), s = createTaskEditor(p);
    s.follow(target("one")); await tick(); await s.pick(); expect(s.view()?.files).toEqual([file]); s.follow(target("two")); await tick(); expect(p.edit).toHaveBeenCalledOnce(); expect(s.view()?.task).toBe("two");
  });
  it("blocks editing an invalid stored draft and concurrent submissions", async () => {
    const pending = deferred<TaskEditorView>(), p = port({ submit: vi.fn().mockReturnValue(pending.promise) }), s = createTaskEditor(p);
    s.follow(target("one")); await tick(); const send = s.submit(); await tick(); await s.submit(); expect(p.submit).toHaveBeenCalledTimes(1); pending.resolve(view("one", { saved: true })); await send;
    const invalid = createTaskEditor(port({ open: vi.fn().mockResolvedValue(view("one", { blocked: true })) })); invalid.follow(target("one")); await tick(); invalid.edit("Lost", 4, 4); expect(invalid.view()?.text).toBe("Imported");
  });
});
