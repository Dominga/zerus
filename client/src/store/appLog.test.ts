import { describe, expect, it, vi } from "vitest";
import { createAppLog, LOG_LINE_CHARS } from "./appLog";

// Task 409: the title bar's log line. The port is the only native seam, so the
// test watches the line, the bounded write and the open call without a window.
const port = (over: Partial<{ latest(): Promise<string | null>; write(l: string, t: string): Promise<void>; open(): Promise<void> }> = {}) => ({
  latest: vi.fn(async () => null as string | null),
  write: vi.fn(async () => {}),
  open: vi.fn(async () => {}),
  ...over,
});

describe("the title bar's log line (task 409)", () => {
  it("shows the line it reads and keeps the last one when the log is empty", async () => {
    const p = port({ latest: async () => "msi-old: reconnecting (attempt 2)" });
    const log = createAppLog(p);
    await log.refresh();
    expect(log.latest()).toBe("msi-old: reconnecting (attempt 2)");
    p.latest = async () => null;
    await log.refresh();
    expect(log.latest()).toBe("msi-old: reconnecting (attempt 2)");
  });

  it("records a change at once and writes one bounded line without line breaks", () => {
    const p = port();
    const log = createAppLog(p);
    log.record("connection", "msi-old: reconnecting\n(attempt 2)");
    expect(log.latest()).toBe("msi-old: reconnecting (attempt 2)");
    expect(p.write).toHaveBeenCalledWith("connection", "msi-old: reconnecting (attempt 2)");
    log.record("warning", "x".repeat(LOG_LINE_CHARS + 50));
    expect(log.latest()).toHaveLength(LOG_LINE_CHARS);
    log.record("notice", "   ");
    expect(log.latest()).toHaveLength(LOG_LINE_CHARS);
  });

  it("opens the whole log, and a refused write keeps the line", async () => {
    const p = port({ write: async () => { throw new Error("the log folder is gone"); } });
    const log = createAppLog(p);
    log.open();
    expect(p.open).toHaveBeenCalledTimes(1);
    log.record("notice", "kept anyway");
    expect(log.latest()).toBe("kept anyway");
  });

  it("reads on its timer while started and stops cleanly", async () => {
    vi.useFakeTimers();
    const p = port({ latest: vi.fn(async () => "line") });
    const log = createAppLog(p, 1000);
    log.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(p.latest).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(2000);
    expect(p.latest).toHaveBeenCalledTimes(3);
    log.stop();
    await vi.advanceTimersByTimeAsync(2000);
    expect(p.latest).toHaveBeenCalledTimes(3);
    vi.useRealTimers();
  });
});
