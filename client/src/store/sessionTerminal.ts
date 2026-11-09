import { createEffect, createRoot } from "solid-js";
import * as bridge from "../bridge";
import { catalog, computerOffline } from "./catalog";
import { change, notify, stillListed, updating } from "./actions";
import { running } from "./batch";
import type { MenuEntry } from "./ui";
import type { Details } from "../types";
import type { Session } from "../ui/sessions/types";
import { STATE_CHANGED, menuState, snapshotOf, terminal, terminalAfterResume, type ActionRow } from "../ui/sessions/actions-ui";
import { toast } from "./notices";
import { tr, trNoop } from "../i18n";

/** How long Resume and open terminal waits for the resumed session to report running. */
export const RESUME_TERMINAL_MS = 30_000;
export const RESUMED_NOT_RUNNING = trNoop("Resumed; terminal not opened: %1 did not report running in time. Open terminal once it does.");
export const RESUMED_CHANGED = trNoop("Resumed; terminal not opened: the session changed. Open its actions again.");
export const RESUMED_NO_TERMINAL = trNoop("Resumed; terminal not opened: Terminal is unavailable for this session now.");

let confirmResume: ((row: ActionRow) => Promise<boolean>) | undefined;
/** Resume and open terminal's one confirmation (ui/sessions/ActionDialogs.tsx) binds itself here. */
export function bindResumeTerminalConfirm(ask: (row: ActionRow) => Promise<boolean>) { confirmResume = ask; }

const unavailable = (s: Pick<ActionRow, "cmd">) =>
  s.cmd === "dsh" || s.cmd === "deepseek" ? tr("This native adapter does not provide Terminal.") : tr("Terminal is unavailable for this session.");

/** The identity-bound attach of a running row: `hgs a NAME --existing --run-id RUN` (tauri/session_folders.rs). */
function openTerminal(row: ActionRow) {
  void bridge.sessionTerminalOpen({ computer: row.computer, name: row.name, run_id: row.runId!, conversation_id: row.conversationId ?? "" }).then((advisory) => {
    if (advisory) toast("Zerus: session opened, but not the way you asked", advisory, "warn");
  }, (error) => toast(tr("Could not open session terminal"), String(error), "error", true));
}

/** External connection, separate from the embedded Terminal and Native UI. */
export function sessionTerminalEntry(s: Session, details?: Details): MenuEntry {
  const row = snapshotOf(s);
  if (s.state !== "running" && s.state !== "archived") return resumeTerminalEntry(s, row, details);
  const reason = computerOffline(s.computer) ? tr("Machine unavailable. Reconnect when it is online.")
    : s.state !== "running" ? tr("Terminal is unavailable for this session.")
    : !s.runId ? tr("Refresh this session's identity before opening its terminal.")
    : !terminal(s, details) ? unavailable(s) : undefined;
  return { label: tr("Open terminal"), icon: "square-terminal", disabled: !!reason, hint: reason,
    action: () => {
      const live = stillListed(row);
      if (reason || !live || live.runId !== row.runId || live.name !== row.name || computerOffline(s.computer)) return;
      openTerminal(row);
    } };
}

/**
 * A saved session's Open terminal (Qt's SessionsWindow.cpp:5466-5476): one
 * confirmation, then the row's own Resume (the same path and checks), and the
 * usual identity-bound terminal once that same session reports running with
 * a run. Never a resume by name: names may repeat, the session's ID or
 * conversation identifies it (zerus-5wmh.13.11.3).
 */
function resumeTerminalEntry(s: Session, row: ActionRow, details?: Details): MenuEntry {
  const online = !computerOffline(s.computer);
  const m = menuState(row, online, updating(s.computer, s.name), details, { context: true, batchRunning: running() });
  const reason = !online ? tr("Machine unavailable. Reconnect when it is online.")
    // Not `terminal`: a saved row has no live pane yet (zerus-5o1ns).
    : !terminalAfterResume(s, details) ? unavailable(s)
    : !s.sessionId && !s.conversationId ? tr("Refresh this session's identity before opening its terminal.")
    : !m.change.enabled ? (m.change.reason ? tr(m.change.reason) : tr("This session cannot be resumed now.")) : undefined;
  return { label: tr("Resume and open terminal"), icon: "square-terminal", disabled: !!reason, hint: reason,
    action: () => {
      const live = stillListed(row);
      if (reason || !live || live.state !== row.state || !confirmResume) return;
      // The answer acts on exactly what the question showed (a live row is
      // updated in place): a session that changed meanwhile is refused, and
      // never turned into another operation such as Pause.
      const shown = snapshotOf(live);
      void confirmResume(shown).then((confirmed) => {
        if (!confirmed) return;
        const now = stillListed(shown);
        if (!now) return;
        if (now.state !== shown.state || now.runId !== shown.runId || (now.conversationId ?? "") !== (shown.conversationId ?? "")) {
          notify("error", tr(STATE_CHANGED), true);
          return;
        }
        void resumeAndOpenTerminal(shown);
      });
    } };
}

/**
 * The same durable session after its resume: the saved row's computer and
 * session ID (else its conversation), now running with a run. "changed" when
 * that session runs another conversation.
 */
export function resumedRow(saved: ActionRow, rows: Iterable<ActionRow>): ActionRow | "changed" | undefined {
  for (const r of rows) {
    if (r.computer !== saved.computer || r.state !== "running" || !r.runId) continue;
    const same = saved.sessionId ? r.sessionId === saved.sessionId : !!saved.conversationId && r.conversationId === saved.conversationId;
    if (!same) continue;
    return saved.conversationId && r.conversationId && r.conversationId !== saved.conversationId ? "changed" : r;
  }
  return undefined;
}

function* listed(): Iterable<ActionRow> {
  for (const id of catalog.order) {
    const r = catalog.rows[id];
    if (r) yield r;
  }
}

/** Watches the live rows, one pass per change, until the session runs or `ms` passes. */
export function waitForResumed(saved: ActionRow, ms = RESUME_TERMINAL_MS, rows: () => Iterable<ActionRow> = listed): Promise<ActionRow | "changed" | undefined> {
  return new Promise((resolve) => createRoot((dispose) => {
    let done = false;
    const finish = (value: ActionRow | "changed" | undefined) => {
      if (done) return;
      done = true; clearTimeout(timer); dispose(); resolve(value);
    };
    const timer = setTimeout(() => finish(undefined), ms);
    createEffect(() => {
      const found = resumedRow(saved, rows());
      // A listed row is updated in place: the answer is the identity that
      // matched, frozen now, never what the row becomes later (zerus-1gima).
      // The terminal opens for that run and conversation, which Rust checks
      // against the live row again.
      if (found) {
        const matched = found === "changed" ? found : snapshotOf(found);
        queueMicrotask(() => finish(matched));
      }
    });
  }));
}

/** After the confirmation: the row's Resume, then the terminal of the session it resumed, or a notice why not. */
export async function resumeAndOpenTerminal(saved: ActionRow, wait = waitForResumed): Promise<void> {
  // The Resume reports its own refusal or failure.
  const out = await change(saved);
  if (out.outcome !== "done") return;
  const live = await wait(saved);
  if (live === "changed") { notify("error", tr(RESUMED_CHANGED), true); return; }
  if (!live) { notify("info", tr(RESUMED_NOT_RUNNING, saved.name), true); return; }
  if (computerOffline(live.computer) || !terminal(live)) { notify("info", tr(RESUMED_NO_TERMINAL), true); return; }
  openTerminal(live);
}
