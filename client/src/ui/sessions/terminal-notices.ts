import type { IMarker, IDecoration, Terminal as XTerm } from "@xterm/xterm";
import { tr } from "../../i18n";
import { systemNotice } from "./notices";
import type { NoticeMode } from "./leadNotices";

// Task 360: the lead's Terminal tab shows the same team-notice turns as
// Activity, folded by xterm markers/decorations over the buffer. The PTY
// stream is never touched: this only reads the rendered buffer and hides rows.

export interface TerminalNoticeRegion {
  /** First buffer line: the pasted "[Zerus team …]" header. */
  start: number;
  /** One past the reply's last line: the next prompt or notice. */
  end: number;
  from: string;
  kind: string;
  /** The notice's first body line, or the reply's first line. */
  line: string;
  /** When the header was first seen, for the fold's time. */
  at: number;
}

const HEADER = /^\s*\[Zerus team(?:\s*·|\])/;
const PROMPT = /^\s*(?:[›❯>]\s?|[$#%]\s|PS [^>\n]*>|[\w.-]+@[\w.-]+:[^$#\n]*[$#])\s*/;

const textLine = (term: XTerm, line: number): string => {
  const handle = term.buffer.active.getLine(line);
  return handle ? handle.translateToString(true) : "";
};

/** The lines a scan reads: enough to catch a pasted header and its reply. */
const SCAN_LINES = 240;

/** Regions visible in `lines`, with prompts closing each notice turn. */
export function terminalNoticeRegions(lines: readonly string[], at = 0, only?: { start: number; end: number }): TerminalNoticeRegion[] {
  const out: TerminalNoticeRegion[] = [];
  for (let i = 0; i < lines.length; i++) {
    if (!HEADER.test(lines[i])) continue;
    let end = i + 1;
    while (end < lines.length && !PROMPT.test(lines[end]) && !HEADER.test(lines[end])) end++;
    if (end >= lines.length) continue; // the reply is still being written
    if (only && (i < only.start || i >= only.end)) continue;
    const header = lines[i].trim();
    const parsed = systemNotice(header);
    let first = "";
    for (let n = i + 1; n < end && !first; n++) first = lines[n].trim();
    out.push({ start: i, end, from: parsed?.from ?? "Zerus", kind: parsed?.kind ?? tr("Zerus notice"), line: first, at });
  }
  return out;
}

/** The fold row's text: sender, kind, first line and time. */
export function terminalNoticeLabel(region: Pick<TerminalNoticeRegion, "from" | "kind" | "line" | "at">): string {
  const time = new Date(region.at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const head = [region.from, region.kind].filter(Boolean).join(" · ");
  return `${head}${region.line ? `: ${region.line}` : ""} · ${time}`;
}

interface Fold {
  marker: IMarker;
  rows: number;
  from: string;
  kind: string;
  line: string;
  at: number;
  decoration?: IDecoration;
  open: boolean;
}

/** Installs the fold over one xterm. Call `dispose` with the terminal. */
export function mountTerminalNotices(term: XTerm, initial: NoticeMode) {
  let mode: NoticeMode = initial;
  let disposed = false;
  const folds: Fold[] = [];
  let scanning = false;
  let frame = 0;

  const rowElements = (): HTMLElement[] => {
    const rows = term.element?.querySelector<HTMLElement>(".xterm-rows");
    return rows ? Array.from(rows.children) as HTMLElement[] : [];
  };
  const cellHeight = (): number => {
    const rows = rowElements();
    const screen = term.element?.querySelector<HTMLElement>(".xterm-screen");
    const height = screen?.getBoundingClientRect().height ?? 0;
    return rows.length ? height / rows.length : 0;
  };

  const showBars = () => {
    for (const fold of folds) {
      const collapse = mode === "collapsed" && !fold.open;
      if (!collapse) {
        fold.decoration?.dispose();
        fold.decoration = undefined;
        continue;
      }
      if (fold.decoration || fold.marker.isDisposed) continue;
      const decoration = term.registerDecoration({
        marker: fold.marker, width: term.cols, height: 1, layer: "top",
      });
      if (!decoration) continue;
      decoration.onRender((element) => {
        element.classList.add("term-fold");
        element.style.height = `${cellHeight()}px`;
        element.innerHTML = "";
        const button = document.createElement("button");
        button.type = "button";
        button.className = "term-fold-btn";
        button.title = tr("Expand this team notice and the lead's reply");
        button.innerHTML = `<span class="term-fold-chev">›</span><span class="term-fold-label"></span>`;
        button.querySelector(".term-fold-label")!.textContent = terminalNoticeLabel(fold);
        button.addEventListener("click", (event) => {
          event.preventDefault();
          event.stopPropagation();
          fold.open = true;
          apply();
        });
        element.appendChild(button);
      });
      decoration.onDispose(() => { if (fold.decoration === decoration) fold.decoration = undefined; });
      fold.decoration = decoration;
    }
  };

  const apply = () => {
    if (disposed) return;
    scan();
    const rows = rowElements();
    const top = term.buffer.active.baseY;
    for (let at = 0; at < rows.length; at++) {
      const line = top + at;
      const fold = folds.find((candidate) => !candidate.marker.isDisposed && line >= candidate.marker.line && line < candidate.marker.line + candidate.rows);
      const row = rows[at];
      row.style.display = "";
      row.style.visibility = "";
      if (!fold || mode === "full" || fold.open) continue;
      const first = line === fold.marker.line;
      if (mode === "silent" || !first) row.style.display = "none";
      else row.style.visibility = "hidden";
    }
    showBars();
  };

  const schedule = () => {
    if (frame || disposed) return;
    frame = requestAnimationFrame(() => { frame = 0; apply(); });
  };

  const scan = () => {
    if (scanning || disposed) return;
    scanning = true;
    try {
      const buffer = term.buffer.active;
      const from = Math.max(0, buffer.length - SCAN_LINES);
      for (let line = from; line < buffer.length; line++) {
        const text = textLine(term, line);
        if (!HEADER.test(text)) continue;
        if (folds.some((fold) => !fold.marker.isDisposed && fold.marker.line === line)) continue;
        // A header still without a prompt: its reply is being written. Keep the
        // marker; the next scan closes it.
        let end = line + 1;
        while (end < buffer.length && !PROMPT.test(textLine(term, end)) && !HEADER.test(textLine(term, end))) end++;
        if (end >= buffer.length) continue;
        const parsed = systemNotice(text.trim());
        let first = "";
        for (let n = line + 1; n < end && !first; n++) first = textLine(term, n).trim();
        const marker = term.registerMarker(line - (buffer.baseY + buffer.cursorY));
        if (!marker) continue;
        folds.push({ marker, rows: end - line, from: parsed?.from ?? "Zerus", kind: parsed?.kind ?? tr("Zerus notice"), line: first, at: Date.now(), open: false });
      }
      folds.sort((a, b) => a.marker.line - b.marker.line);
    } finally {
      scanning = false;
    }
  };

  const subscriptions = [
    term.onWriteParsed(schedule),
    term.onRender(schedule),
    term.onScroll(schedule),
  ];
  schedule();
  return {
    setMode(next: NoticeMode) {
      mode = next;
      for (const fold of folds) { fold.decoration?.dispose(); fold.decoration = undefined; }
      apply();
    },
    dispose() {
      disposed = true;
      if (frame) cancelAnimationFrame(frame);
      for (const subscription of subscriptions) subscription.dispose();
      for (const fold of folds) { fold.decoration?.dispose(); fold.marker.dispose(); }
      folds.length = 0;
      for (const row of rowElements()) { row.style.display = ""; row.style.visibility = ""; }
    },
  };
}


