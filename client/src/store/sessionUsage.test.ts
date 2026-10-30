import { beforeEach, describe, expect, it, vi } from "vitest";
import { emptyUsage, type SessionUsageRef, type Usage } from "../types";
const bridge = vi.hoisted(() => ({ sessionUsage: vi.fn(), onSessionUsage: vi.fn() }));
vi.mock("../bridge", () => bridge);
import { followSessionUsage, refreshSessionUsage, sessionUsage, startSessionUsage, stopSessionUsage } from "./sessionUsage";

const target = (): SessionUsageRef => ({ host: "mac", name: "codex/sample-project/a", run_id: "r1" });
const usage = (): Usage => ({ ...emptyUsage("codex"), id: "bound-account", status: "ok", windows: [{ id: "5h", label: "", minutes: 300, used: 100, resets_at: 5000 }] });
const tick = async () => { for (let i = 0; i < 6; i++) await Promise.resolve(); };
beforeEach(async () => {
  stopSessionUsage(); bridge.sessionUsage.mockResolvedValue(emptyUsage()); followSessionUsage(null); await tick();
  bridge.sessionUsage.mockReset(); bridge.onSessionUsage.mockReset();
});
describe("shared session usage in the footer", () => {
  it("the same binding keeps its snapshot without another read or a loading gap", async () => {
    bridge.sessionUsage.mockResolvedValue(usage());
    followSessionUsage(target(), "codex"); await tick();
    const shown = sessionUsage(); followSessionUsage({ ...target() }, "codex"); await tick();
    expect(bridge.sessionUsage).toHaveBeenCalledExactlyOnceWith(target(), false);
    expect(sessionUsage()).toBe(shown); expect(sessionUsage().windows).toHaveLength(1);
  });
  it("explicit refresh retains the warning values while its reply is pending", async () => {
    bridge.sessionUsage.mockResolvedValueOnce(usage()); followSessionUsage(target(), "codex"); await tick();
    let resolve!: (value: Usage) => void;
    bridge.sessionUsage.mockReturnValue(new Promise((r) => { resolve = r; }));
    const pending = refreshSessionUsage(true);
    expect(bridge.sessionUsage).toHaveBeenLastCalledWith(target(), true);
    expect(sessionUsage().windows[0].used).toBe(100);
    followSessionUsage(target(), "codex"); expect(sessionUsage().windows[0].used).toBe(100);
    resolve({ ...usage(), refreshing: true }); await pending;
    expect(sessionUsage().refreshing).toBe(true);
  });
  it("a new run clears old account data and drops a late reply", async () => {
    let resolve!: (value: Usage) => void;
    bridge.sessionUsage.mockReturnValueOnce(new Promise((r) => { resolve = r; })).mockResolvedValueOnce({ ...usage(), id: "new-account" });
    followSessionUsage(target(), "codex"); followSessionUsage({ ...target(), run_id: "r2" }, "codex"); await tick();
    resolve(usage()); await tick(); expect(sessionUsage().id).toBe("new-account");
  });
  it("failed refreshes keep the last values, and shared queue events pull without forcing", async () => {
    bridge.sessionUsage.mockResolvedValue(usage()); followSessionUsage(target(), "codex"); await tick();
    bridge.sessionUsage.mockRejectedValueOnce(new Error("offline")); await refreshSessionUsage(true);
    expect(sessionUsage().windows[0].used).toBe(100); expect(sessionUsage().refresh_error).toBe(true);
    let changed!: () => void; const stop = vi.fn();
    bridge.onSessionUsage.mockImplementation((fn) => { changed = fn; return stop; });
    startSessionUsage(); startSessionUsage(); changed(); await tick();
    expect(bridge.onSessionUsage).toHaveBeenCalledTimes(1); expect(bridge.sessionUsage).toHaveBeenLastCalledWith(target(), false);
    stopSessionUsage(); expect(stop).toHaveBeenCalledTimes(1);
  });
});
