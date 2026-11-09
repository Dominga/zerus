import { describe, expect, it } from "vitest";
import { lazyView, preloadQueued } from "./lazy-view";

// zerus-ycycu: rarely opened views load outside the entry chunk and are
// fetched one at a time when the window is idle, so a first open is instant.
describe("lazily loaded views", () => {
  it("preloads registered views one at a time, each after the previous settles, and keeps going past a failure", async () => {
    const calls: string[] = [];
    const deferred: Record<string, (fail?: boolean) => void> = {};
    const view = (name: string) => lazyView(() => new Promise<{ default: () => null }>((resolve, reject) => {
      calls.push(name);
      deferred[name] = (fail) => (fail ? reject(new Error(name)) : resolve({ default: () => null }));
    }));
    view("first"); view("second"); view("third");
    const waits: (() => void)[] = [];
    preloadQueued((run) => waits.push(run));
    expect(calls).toEqual([]);
    waits.shift()!();
    expect(calls).toEqual(["first"]);
    deferred.first(true);
    await new Promise((r) => setTimeout(r, 0));
    waits.shift()!();
    expect(calls).toEqual(["first", "second"]);
    deferred.second();
    await new Promise((r) => setTimeout(r, 0));
    waits.shift()!();
    expect(calls).toEqual(["first", "second", "third"]);
    deferred.third();
    await new Promise((r) => setTimeout(r, 0));
    waits.shift()?.();
    expect(calls).toHaveLength(3);
  });

  it("loads again on first use after a failed preload, and loads only once after a good one", async () => {
    let attempts = 0;
    const view = lazyView(async () => { attempts++; if (attempts === 1) throw new Error("offline"); return { default: () => null }; });
    const waits: (() => void)[] = [];
    preloadQueued((run) => waits.push(run));
    waits.shift()!();
    await new Promise((r) => setTimeout(r, 0));
    expect(attempts).toBe(1);
    await view.preload();
    expect(attempts).toBe(2);
    await view.preload();
    expect(attempts).toBe(2);
  });

  it("picks a named export", async () => {
    const Named = () => null;
    const view = lazyView(async () => ({ Named }), (m) => m.Named);
    expect((await view.preload()).default).toBe(Named);
  });
});
