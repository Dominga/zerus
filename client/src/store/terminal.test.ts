import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createTerminal, RESIZE_MS, type Screen, type TerminalPort, type Timers } from "./terminal";
import type { PageVisibility } from "./visibility";
import type { TerminalStatus, TerminalTarget } from "../types";

const target = (run = "run-1"): TerminalTarget => ({ computer: "mac", name: "claude/sample-project/review", run_id: run, machine: "studio" });
const ok: TerminalStatus = {
  supported: true, available: true, reason: "", idle: "Connect to use the session's live terminal",
  connecting: "Connecting to claude/sample-project/review…", disconnected: "Disconnected — the session continues on studio",
};

function fakes() {
  const calls: string[] = [];
  let onEvent: ((m: ArrayBuffer | { kind: "finished"; code: number; status: string }) => void) | undefined;
  let changed = true;
  let failWrite: string | undefined;
  const port: TerminalPort = {
    connect: async (_view, t, cols, rows, handler) => { calls.push(`connect ${t.run_id} ${cols}x${rows}`); onEvent = handler; return { generation: 7, status: `Connected: ${t.name}`, reused: false }; },
    write: async (_view, generation, bytes) => { calls.push(`write ${generation} ${new TextDecoder().decode(bytes)}`); if (failWrite) throw failWrite; },
    resize: async (_view, generation, cols, rows) => { calls.push(`resize ${generation} ${cols}x${rows}`); },
    ack: (_view, generation) => { calls.push(`ack ${generation}`); },
    disconnect: async () => { calls.push("disconnect"); return "Disconnected — the session continues on studio"; },
    retarget: async (_view, t) => { calls.push(`retarget ${t.run_id}`); return changed; },
  };
  let clock = 0;
  const pending: { at: number; fn: () => void }[] = [];
  const timers: Timers = { now: () => clock, later: (fn, ms) => { const job = { at: clock + ms, fn }; pending.push(job); return () => pending.splice(pending.indexOf(job), 1); } };
  const advance = (ms: number) => {
    clock += ms;
    for (const job of pending.filter((j) => j.at <= clock)) { pending.splice(pending.indexOf(job), 1); job.fn(); }
  };
  const written: string[] = [];
  let resets = 0;
  const screen: Screen = { write: (bytes, done) => { written.push(new TextDecoder().decode(bytes)); done(); }, reset: () => { resets++; }, size: () => ({ cols: 100, rows: 30 }) };
  let hidden = false;
  const listeners: (() => void)[] = [];
  const visibility: PageVisibility = { hidden: () => hidden, onChange: (l) => { listeners.push(l); return () => {}; } };
  const setHidden = (v: boolean) => { hidden = v; for (const l of listeners) l(); };
  return { visibility, setHidden, calls, port, timers, advance, screen, written, resets: () => resets, emit: (m: Parameters<NonNullable<typeof onEvent>>[0]) => onEvent?.(m), setChanged: (v: boolean) => { changed = v; }, setFailWrite: (v?: string) => { failWrite = v; } };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("terminal store", () => {
  it("connects when the tab shows and acknowledges each write to the screen", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      expect(t.status()).toBe(ok.idle);
      t.show(f.screen);
      expect(t.status()).toBe(ok.connecting);
      await tick();
      expect(f.calls).toEqual(["retarget run-1", "connect run-1 100x30"]);
      expect(t.status()).toBe("Connected: claude/sample-project/review");
      f.emit(new TextEncoder().encode("hello").buffer);
      expect(f.written).toEqual(["hello"]);
      expect(f.calls.at(-1)).toBe("ack 7");
    });
  });

  it("a hidden view still acknowledges, so the attach never stalls", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      t.hide();
      f.emit(new TextEncoder().encode("while hidden").buffer);
      expect(f.written).toEqual([]);
      expect(f.calls.at(-1)).toBe("ack 7");
    });
  });

  it("resizes are throttled to one per 100 ms plus the trailing one", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      for (let i = 0; i < 200; i++) { t.resized(80 + (i % 7), 24); if (i % 2) f.advance(1); }
      t.resized(120, 40);
      f.advance(RESIZE_MS);
      f.advance(RESIZE_MS);
      const resizes = f.calls.filter((c) => c.startsWith("resize"));
      expect(resizes.length).toBeLessThanOrEqual(3);
      expect(resizes.at(-1)).toBe("resize 7 120x40");
    });
  });

  it("writes keep their order and a refused one shows its text", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      t.input(new TextEncoder().encode("a"));
      t.input(new TextEncoder().encode("b"));
      await tick(); await tick();
      expect(f.calls.filter((c) => c.startsWith("write"))).toEqual(["write 7 a", "write 7 b"]);
      f.setFailWrite("Terminal input queue is full; wait before pasting more text.");
      t.input(new TextEncoder().encode("x".repeat(10)));
      await tick(); await tick();
      expect(t.status()).toBe("Terminal input queue is full; wait before pasting more text.");
    });
  });

  it("a changed target clears the screen and reconnects only while shown", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      const before = f.resets();
      await t.setTarget(target("run-2"), ok);
      await tick();
      expect(f.resets()).toBeGreaterThan(before);
      expect(f.calls.at(-1)).toBe("connect run-2 100x30");
      f.setChanged(false);
      t.hide();
      await t.setTarget(target("run-2"), ok);
      expect(f.calls.filter((c) => c.startsWith("connect")).length).toBe(2);
    });
  });

  it("losing availability disconnects with the reason; a finish shows Qt's status", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      f.emit({ kind: "finished", code: 2, status: "Attach client exited (2) — session output is preserved above" });
      expect(t.status()).toBe("Attach client exited (2) — session output is preserved above");
      expect(t.connected()).toBe(false);
      t.show(f.screen);
      await tick();
      f.setChanged(false);
      await t.setTarget(target(), { ...ok, available: false, reason: "Machine unavailable. Reconnect when it is online." });
      expect(f.calls.at(-1)).toBe("disconnect");
      expect(t.status()).toBe("Machine unavailable. Reconnect when it is online.");
    });
  });

  it("a hidden window disconnects and showing it connects again while the tab shows", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      expect(t.connected()).toBe(true);
      f.setHidden(true);
      await tick();
      expect(f.calls.at(-1)).toBe("disconnect");
      expect(t.connected()).toBe(false);
      expect(t.status()).toBe("Disconnected — the session continues on studio");
      await t.connect();
      expect(f.calls.filter((c) => c.startsWith("connect")).length).toBe(1);
      f.setHidden(false);
      await tick();
      expect(f.calls.at(-1)).toBe("connect run-1 100x30");
      expect(t.connected()).toBe(true);
    });
  });

  it("a window shown again does not connect while the tab is hidden", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      t.hide();
      f.setHidden(true);
      await tick();
      f.setHidden(false);
      await tick();
      expect(f.calls.filter((c) => c.startsWith("connect")).length).toBe(1);
      expect(t.connected()).toBe(false);
    });
  });

  it("hiding only the tab keeps the attach", async () => {
    const f = fakes();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      t.hide();
      await tick();
      expect(f.calls).not.toContain("disconnect");
      expect(t.connected()).toBe(true);
    });
  });
});
