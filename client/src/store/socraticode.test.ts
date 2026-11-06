import { createRoot } from "solid-js";
import { describe, expect, it } from "vitest";
import { createSocratiCode, type SocratiCodeDeps, type SocratiCodePanel } from "./socraticode";

// WhiteWh fork: the SocratiCode card's store (tray/src/SocratiCodePanel.h).
// A read is on demand; a failed read keeps the last status and adds its error;
// an action's new status arrives after Qt's delay; a replay never acts.

const panel = (over: Partial<SocratiCodePanel> = {}): SocratiCodePanel => ({
  state: { kind: "not_indexed", qdrant: true, startable: true },
  details: [], action: { kind: "start", fresh: true, qdrant: true }, connect: false, stop: false, poll: false,
  ...over,
});

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function setup(options: {
  status?: () => Promise<SocratiCodePanel>;
  act?: () => Promise<{ installed: boolean }>;
} = {}) {
  const calls: { command: string; args: unknown[] }[] = [];
  const later: { run: () => void; ms: number }[] = [];
  const bridge: SocratiCodeDeps["bridge"] = {
    socraticodeStatus: (host, path, native) => {
      calls.push({ command: "status", args: [host, path, native] });
      return (options.status ?? (() => Promise.resolve(panel())))();
    },
    socraticodeAct: (action, host, path, native) => {
      calls.push({ command: "act", args: [action, host, path, native] });
      return (options.act ?? (() => Promise.resolve({ installed: true })))();
    },
  };
  let dispose!: () => void;
  const store = createRoot((end) => {
    dispose = end;
    return createSocratiCode({ bridge, later: (run, ms) => later.push({ run, ms }) });
  });
  return { store, calls, later, dispose };
}

describe("SocratiCode card", () => {
  it("reads the selected folder's status, once per context", async () => {
    const t = setup();
    expect(t.store.panel().state).toEqual({ kind: "not_checked" });
    t.store.open("", "D:\\repo", true);
    expect(t.store.busy()).toBe(true);
    expect(t.store.panel().state).toEqual({ kind: "checking" });
    await flush();
    expect(t.calls).toEqual([{ command: "status", args: ["", "D:\\repo", true] }]);
    expect(t.store.data()?.state).toEqual({ kind: "not_indexed", qdrant: true, startable: true });
    expect(t.store.busy()).toBe(false);
    // The same context is not read again; the native switch is part of it.
    t.store.open("", "D:\\repo", true);
    expect(t.calls).toHaveLength(1);
    t.store.open("", "D:\\repo", false);
    expect(t.calls).toHaveLength(2);
    expect(t.calls[1]).toEqual({ command: "status", args: ["", "D:\\repo", false] });
    t.dispose();
  });

  it("keeps the last status and shows a failed read as its error", async () => {
    let fail = false;
    const t = setup({
      status: () => fail ? Promise.reject("D:\\repo is not a folder.") : Promise.resolve(panel({ connect: true })),
    });
    t.store.open("", "D:\\repo", false);
    await flush();
    expect(t.store.data()?.connect).toBe(true);
    fail = true;
    t.store.read();
    await flush();
    expect(t.store.error()).toBe("D:\\repo is not a folder.");
    expect(t.store.data()?.connect).toBe(true);
    expect(t.store.panel().state).toEqual(panel({ connect: true }).state);
    // With no answer held, a failure is the panel's Unavailable state.
    t.store.close();
    t.store.open("", "D:\\repo", false);
    await flush();
    expect(t.store.panel().state).toEqual({ kind: "unavailable" });
    t.dispose();
  });

  it("runs the one next step and re-reads after Qt's delay", async () => {
    const t = setup();
    t.store.open("", "D:\\repo", true);
    await flush();
    expect(await t.store.act("start")).toBe(true);
    expect(t.calls.at(-1)).toEqual({ command: "act", args: ["start", "", "D:\\repo", true] });
    expect(t.later).toHaveLength(1);
    expect(t.later[0].ms).toBe(1500);
    t.later[0].run();
    await flush();
    expect(t.calls.at(-1)).toEqual({ command: "status", args: ["", "D:\\repo", true] });
    expect(await t.store.act("connect")).toBe(true);
    expect(t.later.at(-1)?.ms).toBe(500);
    t.dispose();
  });

  it("reports an install that did not install with Qt's sentence", async () => {
    const t = setup({ act: () => Promise.resolve({ installed: false }) });
    t.store.open("", "D:\\repo", true);
    await flush();
    await t.store.act("install");
    expect(t.store.error()).toBe("The plugin was not installed. Run: claude plugin install socraticode@socraticode");
    await t.store.act("install-qdrant");
    expect(t.store.error()).toBe("Qdrant was not installed.");
    t.dispose();
  });

  it("shows an action's failure and never retries it", async () => {
    let runs = 0;
    const t = setup({ act: () => { runs += 1; return Promise.reject("SocratiCode needs Node.js with npx on this computer."); } });
    t.store.open("", "D:\\repo", true);
    await flush();
    expect(await t.store.act("start")).toBe(false);
    expect(runs).toBe(1);
    expect(t.store.error()).toBe("SocratiCode needs Node.js with npx on this computer.");
    expect(t.store.acting()).toBe(false);
    t.dispose();
  });

  it("a closed card drops its answer and reads nothing", async () => {
    let answer: ((view: SocratiCodePanel) => void) | undefined;
    const t = setup({ status: () => new Promise<SocratiCodePanel>((resolve) => { answer = resolve; }) });
    t.store.open("", "D:\\repo", true);
    t.store.close();
    answer?.(panel());
    await flush();
    expect(t.store.data()).toBeUndefined();
    expect(t.store.context()).toBeUndefined();
    t.store.read();
    expect(t.calls).toHaveLength(1);
    t.dispose();
  });
});
