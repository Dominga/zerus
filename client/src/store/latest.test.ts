import { afterEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { latest, useNow } from "./latest";

describe("latest view revisions", () => {
  it("adopts a newer computation even from an older request", async () => {
    const got: number[] = [];
    const pull = latest<{ revision: number }>((view) => got.push(view.revision));
    let finish!: (view: { revision: number }) => void;
    const first = pull(() => new Promise((resolve) => { finish = resolve; }));
    await pull(async () => ({ revision: 1 }));
    finish({ revision: 2 });
    await first;
    expect(got).toEqual([1, 2]);
  });

  it("rejects a lower computation even from the latest request", async () => {
    const got: number[] = [];
    const pull = latest<{ revision: number }>((view) => got.push(view.revision));
    await pull(async () => ({ revision: 2 }));
    await pull(async () => ({ revision: 1 }));
    await pull(async () => ({ revision: 2 }));
    expect(got).toEqual([2]);
  });

  it("keeps versioned state when an older snapshot has no valid revision", async () => {
    const got: string[] = [];
    const pull = latest<{ revision?: number; text: string }>((view) => got.push(view.text));
    await pull(async () => ({ revision: 1, text: "current" }));
    for (const revision of [undefined, -1, 1.5, Number.POSITIVE_INFINITY]) {
      await pull(async () => ({ revision, text: "old" }));
    }
    expect(got).toEqual(["current"]);
  });
});

describe("useNow", () => {
  afterEach(() => vi.useRealTimers());

  it("ticks every interval and stops with its owner", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    createRoot((dispose) => {
      const now = useNow(15_000);
      expect(now()).toBe(1000);
      vi.advanceTimersByTime(15_000);
      expect(now()).toBe(1015);
      expect(vi.getTimerCount()).toBe(1);
      dispose();
      expect(vi.getTimerCount()).toBe(0);
      vi.advanceTimersByTime(30_000);
      expect(now()).toBe(1015);
    });
  });
});
