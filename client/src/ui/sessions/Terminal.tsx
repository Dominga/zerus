import { createEffect, createSignal, onCleanup, onMount } from "solid-js";
import type { Terminal as XTerm } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { Icon } from "../lib/icons";
import { contentScale } from "../../store/ui";
import { skin } from "../../skin/runtime";
import { terminal } from "../../store/terminal";

// xterm.js 6 loads when Terminal first opens. Linux keeps the DOM renderer;
// macOS and Windows add the WebGL addon and fall back to DOM when its
// context is lost. Scrollback is Qt's MaxHistoryLines (5000).

const encoder = new TextEncoder();
const webgl = typeof navigator !== "undefined" && /Mac|Windows/.test(navigator.userAgent);
const SCROLLBACK = 5000;

function theme() {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return { background: v("--z-color-terminal-bg"), foreground: v("--z-color-terminal-fg"), cursor: v("--z-color-accent") };
}

export function TerminalTab() {
  let host!: HTMLDivElement;
  const [live, setLive] = createSignal<{ term: XTerm; measure: () => void } | undefined>();
  let gone = false;
  let teardown: (() => void) | undefined;
  // Registered synchronously: the owner is gone once the imports below resolve.
  onCleanup(() => { gone = true; teardown?.(); });

  // Reactive inputs apply once the terminal exists.
  createEffect(() => {
    const l = live();
    if (!l) return;
    skin();
    queueMicrotask(() => { l.term.options.theme = theme(); });
  });
  createEffect(() => {
    const l = live();
    if (!l) return;
    l.term.options.fontSize = Math.round(13 * contentScale());
    l.measure();
  });

  onMount(async () => {
    const [{ Terminal }, { FitAddon }] = await Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit"), import("@xterm/xterm/css/xterm.css")]);
    if (gone) return;
    const css = getComputedStyle(document.documentElement);
    const term = new Terminal({ fontFamily: css.getPropertyValue("--z-font-mono"), fontSize: Math.round(13 * contentScale()), cursorBlink: true, scrollback: SCROLLBACK, theme: theme() });
    const fit: FitAddon = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    if (webgl) {
      try {
        const { WebglAddon } = await import("@xterm/addon-webgl");
        if (gone) { term.dispose(); return; }
        const gl = new WebglAddon();
        gl.onContextLoss(() => gl.dispose());
        term.loadAddon(gl);
      } catch { /* the DOM renderer stays */ }
    }
    const measure = () => { if (host.clientWidth > 0 && host.clientHeight > 0) fit.fit(); };
    measure();
    term.onData((data) => terminal.input(encoder.encode(data)));
    term.onBinary((data) => terminal.input(Uint8Array.from(data, (c) => c.charCodeAt(0) & 255)));
    term.onResize(({ cols, rows }) => terminal.resized(cols, rows));
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    teardown = () => { terminal.hide(); observer.disconnect(); term.dispose(); };
    setLive({ term, measure });
    terminal.show({ write: (bytes, done) => term.write(bytes, done), reset: () => term.reset(), size: () => ({ cols: term.cols, rows: term.rows }) });
    term.focus();
  });

  const connected = () => terminal.connected();
  const toggleTip = () => (connected() ? "Disconnect terminal. The session keeps running" : terminal.ready() ? "Connect terminal" : terminal.status());
  return (
    <div class="term-tab">
      <div class="term-bar">
        <span class="state-dot" classList={{ working: connected() }} />
        <span class="ell" title={terminal.status()}>{terminal.status()}</span>
        <span class="grow" />
        <button class="icon-btn" data-tip="Reconnect terminal" aria-label="Reconnect terminal" disabled={!terminal.ready()} onClick={() => { void terminal.reconnect(); }}>
          <Icon name="refresh-cw" size={15} />
        </button>
        <button class="icon-btn" data-tip={toggleTip()} aria-label={connected() ? "Disconnect terminal" : "Connect terminal"} disabled={!connected() && !terminal.ready()}
          onClick={() => { void terminal.toggle(); }}>
          <Icon name={connected() ? "unplug" : "plug"} size={15} />
        </button>
      </div>
      <div class="term-host" ref={host} />
    </div>
  );
}
