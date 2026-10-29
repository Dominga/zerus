import { afterEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { useNow } from "./latest";

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
