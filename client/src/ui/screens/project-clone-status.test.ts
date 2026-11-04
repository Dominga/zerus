import { afterEach, describe, expect, it, vi } from "vitest";
import { watchBusyClone } from "./project-clone-status";
const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
afterEach(() => vi.useRealTimers());
describe("reattached clone status", () => {
  it("coalesces busy reads and stops when the original step settles", async () => {
    vi.useFakeTimers();
    let release!: (value: { busy: boolean }) => void;
    const read = vi.fn(() => new Promise<{ busy: boolean }>((resolve) => { release = resolve; }));
    const update = vi.fn();
    const stop = watchBusyClone(read, update);
    vi.advanceTimersByTime(900); expect(read).toHaveBeenCalledTimes(1);
    release({ busy: true }); await tick();
    vi.advanceTimersByTime(150); expect(read).toHaveBeenCalledTimes(2);
    release({ busy: false }); await tick();
    vi.advanceTimersByTime(1500);
    expect(read).toHaveBeenCalledTimes(2);
    expect(update.mock.calls).toEqual([[{ busy: true }], [{ busy: false }]]);
    stop();
  });
  it("ignores a late read after close or a new explicit step", async () => {
    vi.useFakeTimers();
    let release!: (value: { busy: boolean }) => void;
    const read = vi.fn(() => new Promise<{ busy: boolean }>((resolve) => { release = resolve; }));
    const update = vi.fn(); const stop = watchBusyClone(read, update);
    vi.advanceTimersByTime(150); stop(); release({ busy: false }); await tick();
    vi.advanceTimersByTime(1500);
    expect(read).toHaveBeenCalledTimes(1); expect(update).not.toHaveBeenCalled();
  });
  it("retries only status reads after a transient read failure", async () => {
    vi.useFakeTimers();
    const read = vi.fn().mockRejectedValueOnce("temporary status failure").mockResolvedValue({ busy: false });
    const update = vi.fn(); watchBusyClone(read, update);
    vi.advanceTimersByTime(150); await tick(); vi.advanceTimersByTime(150); await tick();
    vi.advanceTimersByTime(1500);
    expect(read).toHaveBeenCalledTimes(2); expect(update).toHaveBeenCalledExactlyOnceWith({ busy: false });
  });
});
