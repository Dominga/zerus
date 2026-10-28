import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { documentVisibility, type PageVisibility } from "./visibility";
import type { TerminalConnected, TerminalFinished, TerminalStatus, TerminalTarget } from "../types";

// The Terminal tab's state (Qt TerminalView): one attach per view, connected
// while the tab shows. Output is written to the screen and acknowledged once
// xterm parsed it, so Rust delivers at most two messages ahead. A hidden view
// acknowledges at once and drops the bytes: Rust keeps what it delivered and a
// view shown again redraws from it. Writes run one at a time, in order.
// A hidden or minimised window disconnects (Qt SessionsWindow::hideEvent calls
// disconnectSession) and showing it again connects while the tab shows.

export const VIEW = "main";
/** At most one resize per this interval reaches Rust, plus the trailing one. */
export const RESIZE_MS = 100;

export interface TerminalPort {
  connect(view: string, target: TerminalTarget, cols: number, rows: number, onEvent: (message: ArrayBuffer | TerminalFinished) => void): Promise<TerminalConnected>;
  write(view: string, generation: number, bytes: Uint8Array): Promise<void>;
  resize(view: string, generation: number, cols: number, rows: number): Promise<void>;
  ack(view: string, generation: number): void;
  disconnect(view: string): Promise<string | null>;
  retarget(view: string, target: TerminalTarget): Promise<boolean>;
}

export interface Screen {
  write(bytes: Uint8Array, done: () => void): void;
  reset(): void;
  size(): { cols: number; rows: number };
}

export interface Timers { now(): number; later(fn: () => void, ms: number): () => void }
const realTimers: Timers = { now: () => performance.now(), later: (fn, ms) => { const id = setTimeout(fn, ms); return () => clearTimeout(id); } };

const message = (error: unknown) => (typeof error === "string" ? error : error instanceof Error ? error.message : String(error));

export function createTerminal(port: TerminalPort, timers: Timers = realTimers, visibility: PageVisibility = documentVisibility) {
  const [phase, setPhase] = createSignal<"idle" | "connecting" | "connected">("idle");
  const [status, setStatus] = createSignal("");
  const [texts, setTexts] = createSignal<TerminalStatus | undefined>();
  let target: TerminalTarget | undefined;
  let screen: Screen | undefined;
  let generation = 0;
  /** Bumps on every connect, disconnect and target change: older Channels are ignored. */
  let serial = 0;
  let writes: Promise<void> = Promise.resolve();
  let pendingSize: { cols: number; rows: number } | undefined;
  let lastResize = Number.NEGATIVE_INFINITY;
  let resizeTimer: (() => void) | undefined;

  const ready = () => !!target && !!texts()?.available;

  const handler = (mine: number) => (m: ArrayBuffer | TerminalFinished) => {
    if (mine !== serial) return;
    if (m instanceof ArrayBuffer || ArrayBuffer.isView(m)) {
      const view = m as ArrayBufferView;
      const bytes = m instanceof ArrayBuffer ? new Uint8Array(m) : new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
      const gen = generation;
      if (screen) screen.write(bytes, () => port.ack(VIEW, gen));
      else port.ack(VIEW, gen);
      return;
    }
    if (m.kind === "finished") {
      serial++;
      generation = 0;
      setPhase("idle");
      setStatus(m.status);
    }
  };

  /** Connects, or rebinds the running attach after the view was mounted again. */
  async function connect() {
    if (!ready() || !screen || phase() === "connecting" || visibility.hidden()) return;
    const mine = ++serial;
    const rebinding = phase() === "connected";
    setPhase("connecting");
    if (!rebinding) setStatus(texts()!.connecting);
    screen.reset();
    const { cols, rows } = screen.size();
    try {
      const c = await port.connect(VIEW, target!, cols, rows, handler(mine));
      if (mine !== serial) return;
      generation = c.generation;
      setPhase("connected");
      setStatus(c.status);
    } catch (error) {
      if (mine !== serial) return;
      setPhase("idle");
      setStatus(message(error));
    }
  }

  async function disconnect() {
    if (phase() === "idle") return;
    serial++;
    generation = 0;
    setPhase("idle");
    const text = await port.disconnect(VIEW).catch(() => null);
    setStatus(text ?? texts()?.disconnected ?? "");
  }

  // Qt's hideEvent: a hidden or minimised window drops the attach; shown again
  // with the tab visible, it connects as showing the tab does.
  visibility.onChange(() => {
    if (visibility.hidden()) void disconnect();
    else if (screen) void connect();
  });

  return {
    phase, status, ready,
    connected: () => phase() === "connected",
    /** The selected session's target and texts (on every change of either). */
    async setTarget(next: TerminalTarget | undefined, nextTexts: TerminalStatus | undefined) {
      setTexts(nextTexts);
      if (!next || !nextTexts) { await disconnect(); target = undefined; setStatus(""); return; }
      const changed = await port.retarget(VIEW, next);
      target = next;
      if (changed) {
        serial++;
        generation = 0;
        setPhase("idle");
        screen?.reset();
        setStatus(nextTexts.available ? nextTexts.idle : nextTexts.reason);
      }
      if (!nextTexts.available) {
        if (phase() !== "idle") { serial++; generation = 0; setPhase("idle"); await port.disconnect(VIEW).catch(() => null); }
        setStatus(nextTexts.reason);
        return;
      }
      if (screen && phase() === "idle" && changed) void connect();
    },
    /** The tab shows `s`: connect (Qt's currentChanged), or redraw a running attach. */
    show(s: Screen) { screen = s; void connect(); },
    /** The tab hides; the attach keeps running. */
    hide() { screen = undefined; },
    connect, disconnect,
    async reconnect() { await disconnect(); await connect(); },
    async toggle() { if (phase() === "idle") await connect(); else await disconnect(); },
    input(bytes: Uint8Array) {
      if (phase() !== "connected" || !bytes.length) return;
      const gen = generation;
      writes = writes.then(() => port.write(VIEW, gen, bytes)).catch((error) => { setStatus(message(error)); });
    },
    resized(cols: number, rows: number) {
      pendingSize = { cols, rows };
      if (resizeTimer) return;
      const wait = Math.max(0, lastResize + RESIZE_MS - timers.now());
      resizeTimer = timers.later(() => {
        resizeTimer = undefined;
        if (!pendingSize || phase() !== "connected") return;
        const size = pendingSize;
        pendingSize = undefined;
        lastResize = timers.now();
        void port.resize(VIEW, generation, size.cols, size.rows).catch(() => {});
      }, wait);
    },
  };
}

export type TerminalStore = ReturnType<typeof createTerminal>;

/** The app's Terminal, on the bridge. */
export const terminal = createTerminal({
  connect: bridge.terminalConnect, write: bridge.terminalWrite, resize: bridge.terminalResize,
  ack: bridge.terminalAck, disconnect: bridge.terminalDisconnect, retarget: bridge.terminalRetarget,
});
