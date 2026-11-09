import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { detectPlatform } from "../../src/store/shortcuts";
import { clipboardKeyHandler, terminalMenu } from "../../src/ui/sessions/terminal-clipboard";

// Two real xterm.js terminals in the engine under test: "with" runs
// Terminal.tsx's copy key handler and right-click menu, "base" runs xterm
// alone. Each records what reaches the agent (onData), the handler's copies,
// and the engine's own copy, paste and menu events.

const on = detectPlatform(navigator.userAgent);
interface Recorded { data: string[]; copies: number; nativeCopies: number; nativePastes: number; menus: string[] }

function mount(id: string, handled: boolean) {
  const host = document.getElementById(id)!;
  const rec: Recorded = { data: [], copies: 0, nativeCopies: 0, nativePastes: 0, menus: [] };
  const term = new Terminal({ cols: 60, rows: 8 });
  term.open(host);
  term.write("selectable text in the fixture\r\n");
  if (handled) term.attachCustomKeyEventHandler(clipboardKeyHandler(on, () => rec.copies++));
  term.onData((data) => rec.data.push(data));
  host.addEventListener("copy", () => rec.nativeCopies++, true);
  // As Terminal.tsx: the paste event is Zerus's, never xterm's own.
  host.addEventListener("paste", (event) => { rec.nativePastes++; event.preventDefault(); event.stopPropagation(); }, true);
  host.addEventListener("contextmenu", (event) => {
    if (!handled) { rec.menus.push("engine"); return; }
    event.preventDefault();
    const items = terminalMenu({ selection: term.hasSelection(), mouseApp: term.modes.mouseTrackingMode !== "none", shift: event.shiftKey, on }, () => {}, () => {});
    rec.menus.push(items ? items.map((item) => ("label" in item ? `${item.label}${item.disabled ? " (disabled)" : ""}` : "")).join(" | ") : "none");
  });
  return { term, rec, host };
}

const terminals = { with: mount("with", true), base: mount("base", false) };
const fixture = {
  platform: on,
  focus(name: "with" | "base") { const t = terminals[name].term; t.selectAll(); t.focus(); },
  point(name: "with" | "base") { const r = terminals[name].host.getBoundingClientRect(); return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + 30) }; },
  snapshot() { return { with: { ...terminals.with.rec, data: [...terminals.with.rec.data] }, base: { ...terminals.base.rec, data: [...terminals.base.rec.data] } }; },
};
(window as unknown as { terminalFixture: typeof fixture }).terminalFixture = fixture;
(window as unknown as { webkit?: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit?.messageHandlers.result.postMessage(JSON.stringify({ stage: "ready", platform: on }));
