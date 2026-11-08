import { createEffect, createRoot, createSignal } from "solid-js";
import * as bridge from "../bridge";
import { catalog } from "./catalog";
import { notify } from "./actions";
import { setDetailTab, setPage, setSelectedKey, setSubagentOpen } from "./ui";
import { tagProblem } from "../ui/sessions/actions-ui";
import { tr } from "../i18n";

// WhiteWh fork: Continue a Claude app session (tray/src/ClaudeAppSessionsDialog.h
// and SessionsWindow::showNewSession 246-254, 3688-3701). The dialog holds the
// list `hgs claude-app ls` returns; Continue sends Qt's exact
// `hgs claude-app resume ID -n NAME --launch-id ID -d [--force]` once, then the
// window forgets the dialog and shows the launched session when it appears.
// The webview parses no hgs JSON: the core projects the rows and their states.

/** Qt's State column codes. */
export type ClaudeAppState = "open_in_app" | "archived" | "stopped" | "no_history" | "folder_missing";

/** One row of the app's list (zerus_client_core::claude_app::Row). */
export interface ClaudeAppSession {
  id: string;
  conversation: string;
  cwd: string;
  title: string;
  suggested_name: string;
  archived: boolean;
  running: boolean;
  running_process: boolean;
  history: boolean;
  folder_exists: boolean;
  /** Seconds since the Unix epoch, as the app recorded it. */
  last_activity: number;
  state: ClaudeAppState;
  /** The dialog's folder, compared as the core compares folders. */
  in_folder: boolean;
}

/** The read (`claude_app_sessions`). */
export interface ClaudeAppView {
  sessions: ClaudeAppSession[];
  truncated: boolean;
  available: boolean;
  folder: string;
  folder_enabled: boolean;
}

export type ClaudeAppTone = "error" | "warn" | "info";

export interface ClaudeAppDeps {
  bridge: Pick<typeof bridge, "claudeAppSessions" | "claudeAppResume">;
  notify(tone: ClaudeAppTone, text: string, sticky?: boolean): void;
  /** Opens the launched session's Activity (Qt's `m_nativeLaunchToOpen`). */
  select(key: string): void;
  /** The listed rows, for the launch id the app waits for. */
  rows(): readonly { key: string; computer: string; launchId?: string }[];
  /** SessionTag::problem, shared with Rename. */
  problem(name: string): string | undefined;
}

export interface ClaudeAppStore {
  /** The open dialog's computer and folder; undefined closed. */
  open(): { host: string; folder: string } | undefined;
  data(): ClaudeAppView | undefined;
  error(): string;
  /** The list read is in flight. */
  busy(): boolean;
  /** Continue's call is in flight. */
  saving(): boolean;
  onlyFolder(): boolean;
  /** The running-session warning is up. */
  asking(): boolean;
  name(): string;
  problem(): string | undefined;
  /** The rows the "This folder only" checkbox leaves. */
  visible(): ClaudeAppSession[];
  selected(): ClaudeAppSession | undefined;
  canContinue(): boolean;
  openDialog(host: string, folder: string): void;
  close(): void;
  refresh(): void;
  setOnlyFolder(on: boolean): void;
  select(id: string): void;
  setName(name: string): void;
  /** Continue: "sent" when the call started, "asked" when the warning shows, "none" when unusable. */
  begin(): "sent" | "asked" | "none";
  /** The warning's answer: true continues with `--force`. */
  answer(force: boolean): boolean;
}

export function createClaudeApp(deps: ClaudeAppDeps): ClaudeAppStore {
  const [open, setOpen] = createSignal<{ host: string; folder: string }>();
  const [data, setData] = createSignal<ClaudeAppView>();
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [saving, setSaving] = createSignal(false);
  const [only, setOnly] = createSignal(false);
  const [selected, setSelected] = createSignal<string>();
  const [name, setName] = createSignal("");
  const [asking, setAsking] = createSignal(false);
  const [launch, setLaunch] = createSignal<{ id: string; host: string }>();
  let request = 0;

  const visible = () => (data()?.sessions ?? []).filter((row) => !only() || row.in_folder);
  const selectedRow = () => visible().find((row) => row.id === selected());
  const problem = () => deps.problem(name());
  const canContinue = () => {
    const row = selectedRow();
    return !!row && row.history && row.folder_exists && !problem() && !busy() && !saving();
  };

  function choose(id: string | undefined) {
    setSelected(id);
    const row = (data()?.sessions ?? []).find((session) => session.id === id);
    if (row) setName(row.suggested_name);
  }

  function keepSelection() {
    const rows = visible();
    if (!rows.some((row) => row.id === selected())) choose(rows[0]?.id);
    else if (!rows.length) setSelected(undefined);
  }

  async function read() {
    const context = open();
    if (!context || busy()) return;
    const mine = ++request;
    setBusy(true);
    try {
      const view = await deps.bridge.claudeAppSessions(context.host, context.folder);
      if (mine !== request) return;
      setData(view);
      setError("");
      keepSelection();
    } catch (failure) {
      if (mine === request) setError(String(failure));
    } finally {
      if (mine === request) setBusy(false);
    }
  }

  function openDialog(host: string, folder: string) {
    request++;
    setOpen({ host, folder });
    setData(undefined);
    setError("");
    setSelected(undefined);
    setName("");
    setAsking(false);
    setBusy(false);
    // Qt checks "This folder only" when New session has a folder.
    setOnly(folder !== "");
    void read();
  }

  function close() {
    request++;
    setOpen(undefined);
    setData(undefined);
    setError("");
    setBusy(false);
    setSaving(false);
    setAsking(false);
  }

  function send(force: boolean): boolean {
    const context = open();
    const row = selectedRow();
    if (!context || !row || saving()) return false;
    setAsking(false);
    setSaving(true);
    const tag = name();
    deps.notify("info", tr("Continuing the Claude app session…"));
    void deps.bridge
      .claudeAppResume(context.host, row.id, tag, force)
      .then((launchId) => {
        setLaunch({ id: launchId, host: context.host });
        deps.notify("info", tr("Session started. Loading activity…"));
      })
      .catch((failure) => {
        setError(String(failure));
        deps.notify("error", tr("Could not start session %1: %2", tag, String(failure)), true);
      })
      .finally(() => setSaving(false));
    return true;
  }

  // A launched session shows once this computer's listing names its launch id;
  // then the window opens its Activity as Qt does.
  createEffect(() => {
    const pending = launch();
    if (!pending) return;
    const row = deps.rows().find((row) => row.launchId === pending.id && row.computer === pending.host);
    if (row) {
      setLaunch(undefined);
      deps.select(row.key);
    }
  });

  return {
    open,
    data,
    error,
    busy,
    saving,
    onlyFolder: only,
    asking,
    name,
    problem,
    visible,
    selected: selectedRow,
    canContinue,
    openDialog,
    close,
    refresh: () => void read(),
    setOnlyFolder(on) {
      setOnly(on);
      keepSelection();
    },
    select: (id) => choose(id),
    setName: (value) => setName(value),
    begin() {
      if (!canContinue()) return "none";
      if (selectedRow()!.running) {
        setAsking(true);
        return "asked";
      }
      return send(false) ? "sent" : "none";
    },
    answer(force) {
      if (!force) {
        setAsking(false);
        return false;
      }
      return send(true);
    },
  };
}

/** The window's one dialog, wired to the app's bridge and session selection. */
export const claudeApp = createRoot(() => createClaudeApp({
  bridge: { claudeAppSessions: bridge.claudeAppSessions, claudeAppResume: bridge.claudeAppResume },
  notify: (tone, text, sticky) => { notify(tone, text, sticky, "claude-app"); },
  select(key) {
    setSubagentOpen(undefined);
    setSelectedKey(key);
    setDetailTab("activity");
    setPage("sessions");
  },
  rows: () => Object.values(catalog.rows),
  problem: tagProblem,
}));
