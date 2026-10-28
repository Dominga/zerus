import { describe, expect, it, vi } from "vitest";
import { machineCounts, machineEntries, MACHINES_WAITING, MIXED_STATE, NATIVE_HINT, NATIVE_LINE, type MachineRow } from "./machine-batch";
import type { MenuEntry } from "../../store/ui";

const row = (over: Partial<MachineRow> = {}): MachineRow => ({
  state: "running", cmd: "claude", bulkLifecycle: true, resumable: true, activity: "idle", processState: "running", ...over,
});
const items = (entries: MenuEntry[]) => entries.filter((e): e is Extract<MenuEntry, { label: string }> => "label" in e);
const input = (rows: MachineRow[], over: Partial<Parameters<typeof machineEntries>[0]> = {}) => ({
  machine: "box", computer: "", online: true, batchRunning: false, rows, start: vi.fn(), ...over,
});

describe("machineCounts (Qt populateBatchActions)", () => {
  it("counts live and saved rows and skips archived ones", () => {
    const c = machineCounts([row(), row(), row({ state: "paused" }), row({ state: "stopped", resumable: false }), row({ state: "archived" })]);
    expect(c).toMatchObject({ live: 2, saved: 1, allIdle: true, nativeControls: false });
  });
  it("is idle only when every live row is resumable, idle and not exited", () => {
    expect(machineCounts([row(), row({ activity: "busy" })]).allIdle).toBe(false);
    expect(machineCounts([row(), row({ resumable: false })]).allIdle).toBe(false);
    expect(machineCounts([row(), row({ processState: "exited" })]).allIdle).toBe(false);
  });
  it("leaves an OpenCode row or one without bulk lifecycle out and notes it", () => {
    const c = machineCounts([row({ cmd: "opencode" }), row({ bulkLifecycle: false }), row()]);
    expect(c).toMatchObject({ live: 1, saved: 0, nativeControls: true });
  });
});

describe("machineEntries", () => {
  it("lists a heading, Pause all and Resume all with their counts", () => {
    const e = machineEntries(input([row(), row({ state: "paused" })]));
    expect(e[0]).toEqual({ heading: "box" });
    const [pause, resume] = items(e);
    expect(pause).toMatchObject({ label: "Pause all (1)", disabled: false });
    expect(resume).toMatchObject({ label: "Resume all in background (1)", disabled: false });
    expect(pause!.hint).toBeUndefined();
  });
  it("disables both with nothing to do", () => {
    const [pause, resume] = items(machineEntries(input([])));
    expect([pause!.label, pause!.disabled, resume!.label, resume!.disabled]).toEqual(["Pause all (0)", true, "Resume all in background (0)", true]);
  });
  it("explains a busy session and keeps Resume all", () => {
    const e = machineEntries(input([row(), row({ activity: "busy" }), row({ state: "paused" })]));
    const labels = items(e).map((x) => x.label);
    expect(labels).toEqual(["Pause all (2)", MIXED_STATE, "Resume all in background (1)"]);
    expect(items(e)[0]!.disabled).toBe(true);
    expect(items(e)[1]!.disabled).toBe(true);
    expect(items(e)[2]!.disabled).toBe(false);
  });
  it("shows no reason without live rows", () => {
    expect(items(machineEntries(input([row({ state: "paused" })]))).map((x) => x.label)).not.toContain(MIXED_STATE);
  });
  it("puts the OpenCode line first and hints both actions", () => {
    const e = items(machineEntries(input([row({ cmd: "opencode" }), row()])));
    expect(e[0]).toMatchObject({ label: NATIVE_LINE, disabled: true });
    expect(NATIVE_LINE).toBe("OpenCode uses individual Pause / Resume controls");
    expect(e[1]).toMatchObject({ label: "Pause all (1)", hint: NATIVE_HINT });
    expect(e.at(-1)).toMatchObject({ hint: NATIVE_HINT });
    expect(NATIVE_HINT).toBe("OpenCode sessions are excluded; use their individual Pause / Resume controls.");
  });
  it("disables everything while the computer is offline", () => {
    const e = items(machineEntries(input([row(), row({ state: "paused" }), row({ cmd: "opencode" }), row({ activity: "busy" })], { online: false })));
    expect(e.every((x) => x.disabled)).toBe(true);
  });
  it("disables the actions while a batch runs", () => {
    const [pause, resume] = items(machineEntries(input([row(), row({ state: "paused" })], { batchRunning: true })));
    expect(pause!.disabled && resume!.disabled).toBe(true);
  });
  it("starts the call with the count and the machine name", () => {
    const start = vi.fn();
    const [pause, resume] = items(machineEntries(input([row(), row(), row({ state: "paused" })], { computer: "mac", machine: "mac mini", start })));
    pause!.action!();
    resume!.action!();
    expect(start).toHaveBeenNthCalledWith(1, "pause_all", 2);
    expect(start).toHaveBeenNthCalledWith(2, "resume_all", 1);
  });
  it("exports the empty-menu line", () => {
    expect(MACHINES_WAITING).toBe("Waiting for machines…");
  });
});
