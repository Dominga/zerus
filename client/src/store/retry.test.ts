import { afterEach, describe, expect, it, vi } from "vitest";
import { anchorRetry, secondsLeft } from "./retry";

const waiting = (serial: number, remaining: number, delay = 10_000) => ({ phase: "waiting", serial, attempt: 2, delay_ms: delay, remaining_ms: remaining });

describe("restart countdown", () => {
  afterEach(() => vi.useRealTimers());

  it("anchors a wait once per serial on the monotonic clock", () => {
    const first = anchorRetry(undefined, waiting(3, 5000), 1000, "7");
    expect(first).toEqual({ phase: "waiting", source: "7", serial: 3, attempt: 2, delayMs: 10_000, dueAt: 6000 });
    expect(secondsLeft(first, 1000)).toBe(5);
    expect(secondsLeft(first, 2400)).toBe(4);
    expect(secondsLeft(first, 3400)).toBe(3);
    expect(secondsLeft(first, 6000)).toBe(0);
    expect(secondsLeft(first, 9000)).toBe(0);
  });

  it("never restarts or extends the same wait when it is reported again", () => {
    const first = anchorRetry(undefined, waiting(3, 5000), 1000, "7")!;
    // A heartbeat 2 s later reports the same wait with a larger remainder
    // (for example a frame built before a pause): the anchor stays.
    const later = anchorRetry(first, waiting(3, 4900), 3000, "7")!;
    expect(later.dueAt).toBe(6000);
    // A report that the wait ends sooner may only bring it earlier.
    const sooner = anchorRetry(later, waiting(3, 1000), 4000, "7")!;
    expect(sooner.dueAt).toBe(5000);
    expect(anchorRetry(sooner, waiting(3, 9000), 4500, "7")!.dueAt).toBe(5000);
  });

  it("ignores wall-clock jumps", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-11-10T12:00:00Z"));
    const first = anchorRetry(undefined, waiting(1, 8000), 100, "")!;
    vi.setSystemTime(new Date("2026-11-10T13:00:00Z"));
    const again = anchorRetry(first, waiting(1, 8000), 600, "")!;
    expect(again.dueAt).toBe(8100);
    expect(secondsLeft(again, 600)).toBe(8);
    vi.setSystemTime(new Date("2026-11-10T11:00:00Z"));
    expect(secondsLeft(again, 1100)).toBe(7);
  });

  it("anchors a new wait or another source afresh", () => {
    const first = anchorRetry(undefined, waiting(3, 5000), 1000, "7")!;
    expect(anchorRetry(first, waiting(4, 2000), 9000, "7")!.dueAt).toBe(11_000);
    // The same serial from a replaced service source is another wait.
    expect(anchorRetry(first, waiting(3, 4000), 2000, "8")!.dueAt).toBe(6000);
  });

  it("keeps no countdown for an attempt, an idle supervisor or an unknown report", () => {
    expect(anchorRetry(undefined, { phase: "attempting", serial: 3, attempt: 2 }, 0)).toEqual({ phase: "attempting", source: "", serial: 3, attempt: 2 });
    expect(secondsLeft(anchorRetry(undefined, { phase: "attempting", serial: 3 }, 0), 0)).toBe(0);
    expect(anchorRetry(undefined, { phase: "idle" }, 0)?.phase).toBe("idle");
    expect(anchorRetry(undefined, { phase: "soon" }, 0)).toBeUndefined();
    expect(anchorRetry(undefined, null, 0)).toBeUndefined();
  });
});
