import { createSignal } from "solid-js";
import { logLatest, logWrite, openLog as openLogFile } from "../bridge";

// Task 409 (owner, 2026-11-08): the title bar's log line shows the client log's
// last line (a connection change or a warning), written as it happens; clicking
// it opens the whole log. The line is bounded, like the log itself: the native
// side keeps 4 MiB plus one older copy and reads only the file's tail.
export const LOG_LINE_CHARS = 300;
export const LOG_REFRESH_MS = 4000;

/** What the title bar's log line needs from the native side. */
export interface AppLogPort {
  latest(): Promise<string | null>;
  write(level: string, text: string): Promise<void>;
  open(): Promise<void>;
}

const clean = (value: string | null | undefined): string =>
  (value ?? "").replace(/\s+/g, " ").trim().slice(0, LOG_LINE_CHARS);

export function createAppLog(port: AppLogPort, everyMs = LOG_REFRESH_MS) {
  const [latest, setLatest] = createSignal("");
  /** Task 424: the level of the recorded line; an error draws it in a muted red. */
  const [level, setLevel] = createSignal("notice");
  let timer: ReturnType<typeof setInterval> | undefined;
  /** Keeps the last line when the read answers nothing (no log yet). */
  const show = (line: string | null | undefined) => {
    const text = clean(line);
    if (text) setLatest(text);
  };
  const refresh = async () => {
    try { show(await port.latest()); } catch { /* the line keeps its last text */ }
  };
  /** Records one event at once, so the title bar shows it before the next read. */
  const record = (level: string, text: string) => {
    const line = clean(text);
    if (!line) return;
    setLatest(line);
    setLevel(level);
    void port.write(level, line).catch(() => { /* the log is not a feature gate */ });
  };
  const open = () => { void port.open().catch(() => { /* the system viewer is best-effort */ }); };
  const start = () => {
    if (timer !== undefined) return;
    void refresh();
    timer = setInterval(() => { void refresh(); }, everyMs);
  };
  const stop = () => {
    if (timer !== undefined) clearInterval(timer);
    timer = undefined;
  };
  return { latest, level, record, open, refresh, start, stop };
}

export const appLog = createAppLog({ latest: logLatest, write: logWrite, open: openLogFile });
