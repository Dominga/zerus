import { For, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import type { Terminal as XTerm } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { Icon } from "../lib/icons";
import { contentScale, selected } from "../../store/ui";
import { skin } from "../../skin/runtime";
import { screenShowsLatest, terminal, type Screen } from "../../store/terminal";
import { tr } from "../../i18n";
import { terminalMenu } from "./terminal-menu";
import { teams, sessionIs } from "../../store/teams";
import { mountTerminalNotices } from "./terminal-notices";
import { leadNoticeMode, setLeadNoticeMode } from "./Activity";
import { NOTICE_MODES, type NoticeMode } from "./leadNotices";

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
  const [folds, setFolds] = createSignal<ReturnType<typeof mountTerminalNotices> | undefined>();
  const [menu, setMenu] = createSignal<{ x: number; y: number } | undefined>();
  const selection = () => live()?.term.getSelection() ?? "";
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

  // Qt's pasteText: the clipboard's text is cleaned by the store and goes
  // through xterm's own paste; xterm's default handler never sees the event.
  const onPaste = (event: ClipboardEvent) => {
    event.preventDefault();
    event.stopPropagation();
    terminal.paste(event.clipboardData?.getData("text/plain") ?? "");
  };

  // Task #247: Qt's TerminalScreen context menu (Copy selection, Paste) over
  // the terminal area; the webview's own menu is never shown.
  const copySelection = () => {
    const text = selection();
    if (text) void navigator.clipboard?.writeText(text).catch(() => undefined);
    setMenu(undefined);
  };
  const pasteClipboard = async () => {
    setMenu(undefined);
    try {
      const text = await navigator.clipboard.readText();
      if (text) terminal.paste(text);
    } catch { /* the clipboard is unavailable */ }
  };

  onMount(async () => {
    host.addEventListener("paste", onPaste, true);
    onCleanup(() => host.removeEventListener("paste", onPaste, true));
    const onContext = (event: MouseEvent) => {
      event.preventDefault();
      event.stopPropagation();
      setMenu({ x: event.clientX, y: event.clientY });
    };
    const closeMenu = () => setMenu(undefined);
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") closeMenu(); };
    host.addEventListener("contextmenu", onContext);
    window.addEventListener("pointerdown", closeMenu);
    window.addEventListener("keydown", onKey);
    onCleanup(() => {
      host.removeEventListener("contextmenu", onContext);
      window.removeEventListener("pointerdown", closeMenu);
      window.removeEventListener("keydown", onKey);
    });
    const [{ Terminal }, { FitAddon }] = await Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit"), import("@xterm/xterm/css/xterm.css")]);
    if (gone) return;
    const css = getComputedStyle(document.documentElement);
    const row = selected();
    const foldMode = row && isLead(row) ? leadNoticeMode(row) : "full";
    const term = new Terminal({ fontFamily: css.getPropertyValue("--z-font-mono"), fontSize: Math.round(13 * contentScale()), cursorBlink: true, scrollback: SCROLLBACK, theme: theme(), allowProposedApi: true });
    const fit: FitAddon = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    if (webgl && foldMode === "full") {
      try {
        const { WebglAddon } = await import("@xterm/addon-webgl");
        const gl = new WebglAddon();
        gl.onContextLoss(() => gl.dispose());
        term.loadAddon(gl);
      } catch { /* the DOM renderer stays */ }
    }
    if (gone) { term.dispose(); return; }
    const measure = () => { if (host.clientWidth > 0 && host.clientHeight > 0) fit.fit(); };
    measure();
    const screen: Screen = {
      write: (bytes, done) => term.write(bytes, done), paste: (text) => term.paste(text), reset: () => term.reset(), size: () => ({ cols: term.cols, rows: term.rows }),
      latest: () => screenShowsLatest(term),
    };
    term.onData((data) => terminal.input(encoder.encode(data), screen));
    term.onBinary((data) => terminal.input(Uint8Array.from(data, (c) => c.charCodeAt(0) & 255), screen));
    term.onResize(({ cols, rows }) => terminal.resized(cols, rows));
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    const fold = mountTerminalNotices(term, foldMode as NoticeMode);
    setFolds(fold);
    teardown = () => { fold.dispose(); setFolds(undefined); terminal.hide(); observer.disconnect(); term.dispose(); };
    setLive({ term, measure });
    terminal.show(screen);
    term.focus();
  });

  createEffect(() => {
    const fold = folds(), row = selected();
    if (fold) fold.setMode(row && isLead(row) ? leadNoticeMode(row) : "full");
  });
  const isLead = (row: ReturnType<typeof selected>): boolean => {
    if (!row) return false;
    const team = teams.teamOf(row);
    return !!team?.members.some((member) => member.lead && sessionIs(member.session, row.computer, row));
  };
  const connected = () => terminal.connected();
  // Qt's updateButtons tooltips; unavailable shows the reason.
  const reason = () => terminal.texts()?.reason ?? "";
  const available = () => !!terminal.texts()?.available;
  const toggleTip = () => (connected() ? tr("Disconnect terminal. The session keeps running") : available() ? tr("Connect terminal") : reason());
  const reconnectTip = () => (available() ? tr("Reconnect terminal. The session keeps running") : reason());
  return (
    <div class="term-tab">
      <div class="term-bar">
        <span class="ell" title={terminal.status()}>{terminal.status()}</span>
        <Show when={selected() && isLead(selected())}>
          <label class="term-notice-mode" title={tr("How the team's notices to the lead and its replies to them show here.")}>
            <span class="muted">{tr("Team notices")}</span>
            <select class="field small" data-notice-mode value={leadNoticeMode(selected()!)} onChange={(event) => setLeadNoticeMode(selected()!, event.currentTarget.value as NoticeMode)}>
              <For each={NOTICE_MODES}>{(mode) => <option value={mode}>{mode === "full" ? tr("Full") : mode === "collapsed" ? tr("Collapsed") : tr("Hidden")}</option>}</For>
            </select>
          </label>
        </Show>
        <span class="grow" />
        <button class="icon-btn" data-tip={reconnectTip()} aria-label={tr("Reconnect terminal")} disabled={!terminal.ready()} onClick={() => { void terminal.reconnect(); }}>
          <Icon name="refresh-cw" size={15} />
        </button>
        <button class="icon-btn" data-tip={toggleTip()} aria-label={connected() ? tr("Disconnect terminal") : tr("Connect terminal")} disabled={!connected() && !terminal.ready()}
          onClick={() => { void terminal.toggle(); }}>
          <Icon name={connected() ? "unplug" : "plug"} size={15} />
        </button>
      </div>
      <div class="term-host" data-shortcut-context="terminal" ref={host} />
      <Show when={menu()}>
        {(at) => (
          <div class="term-menu" role="menu" style={{ left: `${at().x}px`, top: `${at().y}px` }} onPointerDown={(event) => event.stopPropagation()}>
            <For each={terminalMenu(selection())}>
              {(item) => (
                <button type="button" role="menuitem" data-terminal-action={item.id} disabled={item.disabled}
                  onClick={() => { if (item.id === "copy") copySelection(); else void pasteClipboard(); }}>
                  {item.label}
                </button>
              )}
            </For>
          </div>
        )}
      </Show>
    </div>
  );
}
