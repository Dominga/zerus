import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { cleanPaste, createTerminal, RESIZE_MS, type Screen, type TerminalPort, type Timers } from "./terminal";
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

function deferred<T>() { let resolve!: (v: T) => void; const p = new Promise<T>((r) => { resolve = r; }); return { p, resolve }; }

// A model of terminal.rs: a connect inserts the view when its blocking task ends;
// a disconnect removes whatever is there when it runs and never waits for a connect.
function rustLike() {
  const views = new Map<string, { generation: number; run: string }>();
  const calls: string[] = [];
  let next = 0;
  let onEvent: ((m: ArrayBuffer) => void) | undefined;
  let remembered = "";
  let holdConnect: ReturnType<typeof deferred<void>> | undefined;
  let holdDisconnect: ReturnType<typeof deferred<void>> | undefined;
  const port: TerminalPort = {
    connect: async (view, t, _c, _r, h) => {
      calls.push(`connect ${t.run_id}`); onEvent = h as (m: ArrayBuffer) => void;
      if (holdConnect) await holdConnect.p;
      const existing = views.get(view);
      if (existing) return { generation: existing.generation, status: "Connected", reused: true };
      const generation = ++next; views.set(view, { generation, run: t.run_id }); return { generation, status: "Connected", reused: false };
    },
    write: async () => {}, resize: async (_v, g, c, r) => { calls.push(`resize ${g} ${c}x${r}`); },
    ack: (_v, g) => { calls.push(`ack ${g}`); },
    disconnect: async (view) => { calls.push("disconnect"); if (holdDisconnect) await holdDisconnect.p; return views.delete(view) ? "Disconnected" : null; },
    retarget: async (view, t) => { calls.push(`retarget ${t.run_id}`); const changed = remembered !== t.run_id; remembered = t.run_id; if (changed) views.delete(view); return changed; },
  };
  let hidden = false; const ls: (() => void)[] = [];
  const visibility: PageVisibility = { hidden: () => hidden, onChange: (l) => { ls.push(l); return () => {}; } };
  let clock = 0; const pending: { at: number; fn: () => void }[] = [];
  const timers: Timers = { now: () => clock, later: (fn, ms) => { const j = { at: clock + ms, fn }; pending.push(j); return () => pending.splice(pending.indexOf(j), 1); } };
  const advance = (ms: number) => { clock += ms; for (const j of pending.filter((x) => x.at <= clock)) { pending.splice(pending.indexOf(j), 1); j.fn(); } };
  let size = { cols: 100, rows: 30 };
  const pasted: string[] = [];
  const written: string[] = [];
  let resets = 0;
  const screen: Screen = { write: (b, done) => { written.push(new TextDecoder().decode(b)); done(); }, paste: (t) => { pasted.push(t); }, reset: () => { resets++; }, size: () => size };
  return {
    views, calls, port, visibility, timers, advance, screen, pasted, written, resets: () => resets,
    emit: (m: string) => onEvent?.(new TextEncoder().encode(m).buffer as ArrayBuffer),
    setHidden: (v: boolean) => { hidden = v; for (const l of ls) l(); },
    setSize: (cols: number, rows: number) => { size = { cols, rows }; },
    holdConnect: () => (holdConnect = deferred<void>()), holdDisconnect: () => (holdDisconnect = deferred<void>()),
  };
}

describe("terminal store ordering", () => {
  it("a window hidden while a connect is in flight ends that attach", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      const hold = f.holdConnect();
      t.show(f.screen);
      f.setHidden(true);
      await tick();
      hold.resolve();
      await tick(); await tick();
      expect(f.views.size).toBe(0);
      expect(t.connected()).toBe(false);
      expect(t.status()).toBe("Disconnected");
      expect(f.calls.indexOf("disconnect")).toBeGreaterThan(f.calls.indexOf("connect run-1"));
    });
  });

  it("hide then show quickly: the late disconnect does not overwrite the new connect", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      const hold = f.holdDisconnect();
      f.setHidden(true);
      f.setHidden(false);
      await tick();
      hold.resolve();
      await tick(); await tick(); await tick();
      expect(t.connected()).toBe(true);
      expect(f.views.size).toBe(1);
      expect(t.status()).toBe("Connected");
    });
  });

  it("a late disconnect does not replace the connecting status of the connect behind it", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      const gate = f.holdDisconnect();
      f.setHidden(true);
      f.setHidden(false);
      const hold = f.holdConnect();
      await tick();
      gate.resolve();
      await tick(); await tick();
      expect(t.phase()).toBe("connecting");
      expect(t.status()).toBe(ok.connecting);
      hold.resolve();
      await tick(); await tick();
      expect(t.status()).toBe("Connected");
    });
  });

  it("a toggle while connecting disconnects after the connect and leaves no attach", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      const hold = f.holdConnect();
      t.show(f.screen);
      await tick();
      expect(t.phase()).toBe("connecting");
      void t.toggle();
      hold.resolve();
      await tick(); await tick(); await tick();
      expect(f.views.size).toBe(0);
      expect(t.connected()).toBe(false);
      expect(t.status()).toBe("Disconnected");
    });
  });

  it("a selection change while connecting leaves only the new session's attach", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target("run-1"), ok);
      const hold = f.holdConnect();
      t.show(f.screen);
      await tick();
      t.leaveSession();
      const second = t.setTarget(target("run-2"), ok);
      hold.resolve();
      await second;
      await tick(); await tick();
      expect([...f.views.values()].map((v) => v.run)).toEqual(["run-2"]);
      expect(t.connected()).toBe(true);
    });
  });

  it("output before the connect resolves is acknowledged with the real generation", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      const hold = f.holdConnect();
      t.show(f.screen);
      await tick();
      f.emit("first"); f.emit("second");
      expect(f.calls.some((c) => c === "ack 0")).toBe(false);
      hold.resolve();
      await tick(); await tick();
      expect(f.calls.filter((c) => c === "ack 1").length).toBe(2);
    });
  });

  it("a resize while connecting reaches Rust once connected", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      const hold = f.holdConnect();
      t.show(f.screen);
      await tick();
      f.setSize(120, 40);
      hold.resolve();
      await tick(); await tick();
      f.advance(RESIZE_MS);
      expect(f.calls).toContain("resize 1 120x40");
    });
  });

  it("a window shown again resumes only an attach that was running or connecting", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      await t.disconnect();
      f.setHidden(true); await tick();
      f.setHidden(false); await tick();
      expect(f.calls.filter((c) => c.startsWith("connect")).length).toBe(1);
      expect(t.connected()).toBe(false);
    });
  });

  it("returning to the page does not connect; opening the tab does", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      t.hide();
      await t.leavePage();
      expect(t.connected()).toBe(false);
      t.show(f.screen);
      await tick();
      expect(t.connected()).toBe(false);
      t.hide();
      t.opened();
      t.show(f.screen);
      await tick();
      expect(t.connected()).toBe(true);
    });
  });

  it("input stops at once when the selection changes", async () => {
    const f = rustLike();
    const writes: string[] = [];
    const port = { ...f.port, write: async (_v: string, g: number, b: Uint8Array) => { writes.push(`${g} ${new TextDecoder().decode(b)}`); } };
    await createRoot(async () => {
      const t = createTerminal(port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      t.input(new TextEncoder().encode("a"));
      t.leaveSession();
      t.input(new TextEncoder().encode("b"));
      await tick(); await tick();
      expect(writes).toEqual(["1 a"]);
    });
  });

  it("a new target stops the old session's input before its retarget is awaited", async () => {
    const f = rustLike();
    const writes: string[] = [];
    const port = { ...f.port, write: async (_v: string, g: number, b: Uint8Array) => { writes.push(`${g} ${new TextDecoder().decode(b)}`); } };
    await createRoot(async () => {
      const t = createTerminal(port, f.timers, f.visibility);
      await t.setTarget(target("run-1"), ok);
      t.show(f.screen);
      await tick();
      void t.setTarget(target("run-2"), ok);
      t.input(new TextEncoder().encode("late"));
      await tick(); await tick();
      expect(writes).toEqual([]);
    });
  });

  it("a stale attach's output is neither shown nor acknowledged", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      await tick();
      await t.disconnect();
      const acks = f.calls.length;
      f.emit("late");
      expect(f.written).toEqual([]);
      expect(f.calls.length).toBe(acks);
    });
  });

  it("a changed target clears the screen even when it cannot connect", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target("run-1"), ok);
      t.show(f.screen);
      await tick();
      const before = f.resets();
      await t.setTarget(target("run-2"), { ...ok, available: false, reason: "Machine unavailable. Reconnect when it is online." });
      expect(f.resets()).toBe(before + 1);
      expect(t.connected()).toBe(false);
    });
  });
});

describe("terminal paste", () => {
  it("drops controls, DEL and C1, keeps tab and line breaks, and turns CRLF into LF", () => {
    expect(cleanPaste("a\x1b[201~b")).toBe("a[201~b");
    expect(cleanPaste("t\tx\r\ny\nz\rw")).toBe("t\tx\ny\nz\rw");
    expect(cleanPaste("a\x00\x07\x7f\u0085\u009fb")).toBe("ab");
    expect(cleanPaste("\u00e9\u4e2d")).toBe("\u00e9\u4e2d");
  });

  it("goes to xterm's paste only when connected, already cleaned", async () => {
    const f = rustLike();
    await createRoot(async () => {
      const t = createTerminal(f.port, f.timers, f.visibility);
      await t.setTarget(target(), ok);
      t.show(f.screen);
      t.paste("early");
      await tick();
      t.paste("a\x1b[201~b\r\nc");
      t.paste("\x1b\x07");
      expect(f.pasted).toEqual(["a[201~b\nc"]);
    });
  });
});
