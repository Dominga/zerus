import { createRoot } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import { createTaskFiles, taskFileCard, type TaskFileCardPort } from "./taskFiles";
import type { Fetched, TaskFileSource } from "../types";

const source = (file: number, sha = "a".repeat(64)): TaskFileSource => ({
  entry: null, file, meta: { name: `f${file}.png`, mime: "image/png", reference: "", size: 3, sha256: sha },
});
const copy = (name: string): Fetched => ({ path: `cache/x/${name}`, name, mime: "image/png", bytes: 3, thumbnail: true });
const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

describe("task files read on their own", () => {
  it("two cards of one file share one read, and a later open reads it again", async () => {
    const read = vi.fn().mockResolvedValue(copy("f1.png"));
    const files = createTaskFiles(read);
    const [a, b] = [files.read("p1", "t1", source(1)), files.read("p1", "t1", source(1))];
    expect(await a).toEqual(await b);
    expect(read).toHaveBeenCalledOnce();
    await files.read("p1", "t1", source(1));
    expect(read).toHaveBeenCalledTimes(2);
  });
  it("previews take turns, and a card no longer shown reads nothing", async () => {
    const finish: ((file: Fetched) => void)[] = [];
    const read = vi.fn(() => new Promise<Fetched>((resolve) => { finish.push(resolve); }));
    const files = createTaskFiles(read);
    let second = true;
    const first = files.preview("p1", "t1", source(1, "1".repeat(64)), () => true);
    const hidden = files.preview("p1", "t1", source(2, "2".repeat(64)), () => second);
    const third = files.preview("p1", "t1", source(3, "3".repeat(64)), () => true);
    await tick();
    expect(read).toHaveBeenCalledOnce();
    second = false;
    finish[0](copy("f1.png"));
    expect(await first).toEqual(copy("f1.png"));
    expect(await hidden).toBeUndefined();
    await tick();
    expect(read).toHaveBeenCalledTimes(2);
    expect(read).toHaveBeenLastCalledWith("p1", "t1", source(3, "3".repeat(64)));
    finish[1](copy("f3.png"));
    expect(await third).toEqual(copy("f3.png"));
  });
  it("a failed preview does not hold up the next", async () => {
    const read = vi.fn().mockRejectedValueOnce(new Error("This attachment changed. Refresh this task.")).mockResolvedValue(copy("f2.png"));
    const files = createTaskFiles(read);
    await expect(files.preview("p1", "t1", source(1, "1".repeat(64)), () => true)).rejects.toThrow("changed");
    expect(await files.preview("p1", "t1", source(2, "2".repeat(64)), () => true)).toEqual(copy("f2.png"));
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
/** A card's file as its page names it: not read yet. */
const listed = (mime: string): Fetched => ({ path: "", name: "f1.png", mime, bytes: 3, thumbnail: false, source: source(1) });
function card(mime: string, removedDuringRead: boolean) {
  const pending = deferred<Fetched>(), read = vi.fn(() => pending.promise), opened: string[] = [], notices: string[] = [];
  const port: TaskFileCardPort = {
    files: createTaskFiles(read), open: async (path) => { opened.push(path); return null; },
    notify: (tone, text) => { notices.push(`${tone} ${text}`); },
  };
  let dispose!: () => void;
  const shown = createRoot((done) => { dispose = done; return taskFileCard(listed(mime), "p1", "t1", port); });
  return { shown, read, opened, notices, dispose, finish: () => { if (removedDuringRead) dispose(); pending.resolve(copy("f1.png")); } };
}

describe("a task file card (zerus-cg5h.28)", () => {
  it("applies its preview only while it is still shown", async () => {
    for (const removed of [false, true]) {
      const c = card("image/png", removed);
      await tick();
      expect(c.read).toHaveBeenCalledOnce();
      c.finish();
      await tick();
      expect(c.shown.copy()).toEqual(removed ? undefined : copy("f1.png"));
      c.dispose();
    }
  });
  it("an open whose read ends after the card was removed neither keeps the copy nor opens the file", async () => {
    for (const removed of [false, true]) {
      const c = card("text/plain", removed);
      await tick();
      expect(c.read).not.toHaveBeenCalled();
      const opening = c.shown.open();
      void c.shown.open();
      await tick();
      expect(c.read).toHaveBeenCalledOnce();
      expect(c.shown.reading()).toBe(true);
      c.finish();
      await opening;
      expect(c.opened).toEqual(removed ? [] : ["cache/x/f1.png"]);
      expect(c.shown.copy()).toEqual(removed ? undefined : copy("f1.png"));
      expect([c.shown.reading(), c.notices]).toEqual([false, []]);
      c.dispose();
    }
  });
});
