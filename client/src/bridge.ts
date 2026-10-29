import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { emptyAccounts, emptyBatch, emptyMachines, emptyProjects, isEmptyBatch, type AccountsView, type MachinesView, type ProjectsView, type Screen, type Action, type Answer, type Batch, type ComposerTab, type ComposerView, type ComputerId, type Details, type EditOutcome, type Inspect, type Outcome, type ReadMark, type SendReply, type SessionRef, type Target, type TerminalConnected, type TerminalFacts, type TerminalFinished, type TerminalStatus, type TerminalTarget, type ArchivePage, type ArchivedActivity, type AttachmentRef, type Fetched } from "./types";
import type { ActivityPort, FollowRequest } from "./store/activity";
import { documentVisibility, type PageVisibility } from "./store/visibility";

// The one path from the Rust side to the stores. The frontend asks for the next
// batch from requestAnimationFrame with one call in flight; `next_batch`
// parks up to 250 ms while nothing is pending, so an idle client does not
// poll. A batch is applied in the frame after it arrives, at most one per
// frame. Outside Tauri (`npm run dev` in a browser) a built-in replay of the
// synthetic fixture stands in, so the UI works without the app.

export type BridgeMode = "tauri" | "replay";

export interface Source extends ActivityPort {
  next(): Promise<Batch>;
}

let source: Source | undefined;

export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** `next_batch` answers with raw JSON bytes (`ipc::Response`), so large batches skip script evaluation. */
function decodeBatch(reply: unknown): Batch {
  if (reply instanceof ArrayBuffer) return JSON.parse(new TextDecoder().decode(reply)) as Batch;
  if (ArrayBuffer.isView(reply)) return JSON.parse(new TextDecoder().decode(reply)) as Batch;
  if (Array.isArray(reply)) return JSON.parse(new TextDecoder().decode(Uint8Array.from(reply as number[]))) as Batch;
  return reply as Batch;
}

const tauriSource: Source = {
  next: () => invoke<unknown>("next_batch").then(decodeBatch),
  follow: (request: FollowRequest) => invoke<void>("follow", { ...request }),
  unfollow: (computer: ComputerId, key: string) => invoke<void>("unfollow", { computer, key }),
};

/** Starts the data source and the per-frame pump; resolves with the mode in use. */
export async function startBridge(onBatch: (b: Batch) => void): Promise<BridgeMode> {
  let mode: BridgeMode = "replay";
  if (inTauri()) {
    try {
      await invoke("start");
      source = tauriSource;
      mode = "tauri";
    } catch (error) {
      // A build without the live feature has no data source of its own.
      console.warn("Zerus Next: no live data in this build, using the built-in replay.", error);
    }
  }
  if (!source) {
    const { createReplay } = await import("./dev/replay");
    source = createReplay();
  }
  pump(source, onBatch);
  return mode;
}

/** What the pump needs from the page; tests pass their own. */
export interface PumpHost {
  /** The next display frame (requestAnimationFrame). */
  frame(callback: () => void): void;
  /** A timer (setTimeout); returns its cancel. */
  later(callback: () => void, ms: number): () => void;
  visibility: PageVisibility;
}

const pageHost: PumpHost = {
  frame: (callback) => { requestAnimationFrame(() => callback()); },
  later: (callback, ms) => { const timer = setTimeout(callback, ms); return () => clearTimeout(timer); },
  visibility: documentVisibility,
};

/** Calls, frames and batches of the pump since it started (Diagnostics and the hidden-window check read them). */
export const pumpStats = { pulls: 0, frames: 0, applied: 0, held: 0 };

/**
 * One `next_batch` in flight. A batch is applied in the next animation frame,
 * which then asks for the following one; an empty answer (the 250 ms park ran
 * out) asks again at once without a frame, so an idle window schedules no
 * frames. While the window is hidden or minimised the pump stops: it asks for
 * nothing and schedules no frame, a batch that arrives meanwhile waits
 * unapplied, and the Rust side keeps folding changes into the next batch
 * (bounded per key). Showing the window applies the waiting batch in its first
 * frame and resumes.
 */
export function pump(from: Source, onBatch: (b: Batch) => void, host: PumpHost = pageHost) {
  /** A batch that arrived while hidden. */
  let held: Batch | undefined;
  /** Stopped while hidden; nothing is in flight or scheduled. */
  let stopped = false;
  const hidden = () => host.visibility.hidden();
  const pull = () => {
    if (hidden()) { stopped = true; return; }
    pumpStats.pulls++;
    from.next().then(
      (reply) => {
        const b = normalize(reply ?? {});
        if (isEmptyBatch(b)) { pull(); return; }
        deliver(b);
      },
      (error) => {
        console.error("Zerus Next: next_batch failed", error);
        host.later(pull, 1000);
      },
    );
  };
  const deliver = (b: Batch) => {
    if (hidden()) { held = b; stopped = true; pumpStats.held++; return; }
    // In the next frame; a window whose frames are throttled (occluded)
    // still applies it within 250 ms, so the stores never fall far behind.
    let done = false;
    const apply = () => {
      if (done) return;
      done = true;
      cancel();
      if (hidden()) { held = b; stopped = true; pumpStats.held++; return; }
      pumpStats.applied++;
      try { onBatch(b); } catch (error) { console.error("Zerus Next: applying a batch failed", error); }
      pull();
    };
    const cancel = host.later(apply, 250);
    pumpStats.frames++;
    host.frame(apply);
  };
  const resume = () => {
    if (hidden() || !stopped) return;
    stopped = false;
    const b = held;
    held = undefined;
    if (b) deliver(b); else pull();
  };
  const unsubscribe = host.visibility.onChange(resume);
  pull();
  return unsubscribe;
}

/**
 * Missing arrays default to empty, so a core that omits empty fields still
 * applies. The grouping, the composer's view and the draft keys are kept: a
 * batch of only one of them is content (Rust sends each once).
 */
export function normalize(b: Partial<Batch>): Batch {
  const e = emptyBatch();
  return {
    upserts: b.upserts ?? e.upserts, removes: b.removes ?? e.removes, replaces: b.replaces ?? e.replaces, computers: b.computers ?? e.computers,
    machines: b.machines ?? [], activity: b.activity ?? e.activity, groups: b.groups ?? [], grouping: b.grouping ?? undefined,
    composer: b.composer ?? undefined, draft_keys: b.draft_keys ?? undefined,
  };
}

/** The Activity port of the current source (follow and unfollow). */
export const activityPort: ActivityPort = {
  follow: (request) => source?.follow(request) ?? Promise.resolve(),
  unfollow: (computer, key) => source?.unfollow(computer, key) ?? Promise.resolve(),
};

/** Process RSS and CPU of the app, when running in Tauri. */
export async function metrics(windowMs?: number): Promise<Record<string, unknown> | undefined> {
  if (!inTauri()) return undefined;
  try { return await invoke<Record<string, unknown>>("metrics", windowMs ? { window_ms: windowMs } : {}); } catch { return undefined; }
}

/** One guarded session action; an Err (a rejected promise) is an unknown outcome. */
export async function runAction(target: Target, action: Action): Promise<Outcome> {
  if (!inTauri()) return { outcome: "failed", detail: "Actions need the desktop app." };
  return invoke<Outcome>("run_action", { target, action });
}

/**
 * Changes the shared notification preferences of a session; resolves with the
 * host's confirmed `preferences`. A rejection carries the reason.
 */
export async function setNotifications(target: Target, muted: boolean, expectedRevision: number | null): Promise<Record<string, unknown>> {
  if (!inTauri()) throw new Error("Actions need the desktop app.");
  return invoke<Record<string, unknown>>("set_notifications", { target, muted, expectedRevision });
}

/** The inspect fields of a session; null when they are unavailable. */
export async function inspectSession(target: Target, archiveId: string | null): Promise<Details | null> {
  if (!inTauri()) return null;
  return (await invoke<Details | null>("inspect_session", { target, archiveId })) ?? null;
}

/** Read marks whose delivery has not been confirmed yet; undefined outside the app, which keeps no list. */
export async function pendingReadMarks(): Promise<ReadMark[] | undefined> {
  if (!inTauri()) return undefined;
  return invoke<ReadMark[]>("pending_read_marks");
}

/** One attention mark of this device (client/core `marks.rs`); a missing field reads as false or "". */
export interface Mark { review: boolean; acknowledged: string }
/**
 * This device's attention marks and the replies it read on hosts without
 * shared read state, by key, and each map's keys oldest written first (a
 * JSON object's key order is the key order, not the write order).
 */
export interface AttentionSnapshot {
  marks: Record<string, Mark>;
  replies: Record<string, string>;
  order?: { marks: string[]; replies: string[] };
}

// The commands below exist only in the live app. Outside Tauri there are no
// marks, an update does nothing and there is no pending list, so the marks
// store keeps its own; a build without the live feature rejects them, and the
// store falls back the same way.

/** The stored marks; the store awaits them before its first update. */
export async function attentionMarks(): Promise<AttentionSnapshot> {
  if (!inTauri()) return { marks: {}, replies: {}, order: { marks: [], replies: [] } };
  return invoke<AttentionSnapshot>("attention_marks");
}

/** Writes or removes (null) only the changed entries; both maps are always sent. */
export async function updateAttentionMarks(marks: Record<string, Mark | null>, replies: Record<string, string | null>): Promise<void> {
  if (!inTauri()) return;
  await invoke<null>("update_attention_marks", { marks, replies });
}

/** Merges read marks into the pending ones and wakes the Rust delivery worker; answers every pending mark (undefined outside the app). */
export async function queueReadMarks(marks: ReadMark[]): Promise<ReadMark[] | undefined> {
  if (!inTauri()) return undefined;
  return invoke<ReadMark[]>("queue_read_marks", { marks });
}

/** `hgs reference` for a session: its `zerus://session?...` URI. */
export async function sessionReference(target: Target, archiveId: string | null): Promise<string> {
  if (!inTauri()) throw "Actions need the desktop app.";
  return invoke<string>("session_reference", { target, archiveId });
}

/** The command that opens the session in a terminal, written for this computer's shell. */
export async function copyCommand(computer: string, name: string, archiveId: string | null): Promise<string> {
  if (!inTauri()) throw "Actions need the desktop app.";
  return invoke<string>("copy_command", { computer, name, archiveId });
}

/**
 * One checked page of a computer's archive (`archive_page`, `hgs [@alias] archives --json`);
 * `request` is the archive store's request number. Rejects with Qt's text for
 * an invalid page, "superseded" when a newer request of that computer or a
 * cancel replaced it, or the hgs failure.
 */
export async function archivePage(computer: string, from: number, until: number, cursor: string, query: string, request: number): Promise<ArchivePage> {
  if (!inTauri()) throw "Archive pages need the desktop app.";
  return invoke<ArchivePage>("archive_page", { computer, from, until, cursor, query, request });
}

/** The text of `archivedActivity` rejecting because that row's read is already running. */
export const ARCHIVED_ACTIVITY_BUSY = "busy";

/**
 * One read of an archived session's recorded Activity after cursor `after`
 * (`archived_activity`, `hgs [@alias] inspect NAME --after N --skip-processes
 * --archive ID`). Rejects with "busy" (`ARCHIVED_ACTIVITY_BUSY`, to ignore)
 * while a read of the same computer, name and archive id runs, else with Qt's
 * failure text.
 */
export async function archivedActivity(computer: string, name: string, archiveId: string, after: number): Promise<ArchivedActivity> {
  if (!inTauri()) throw "Archived Activity needs the desktop app.";
  return invoke<ArchivedActivity>("archived_activity", { computer, name, archiveId, after });
}

/**
 * Cancels every archive page request numbered `through` or lower (Qt's
 * cancelArchivePages), also one that has not started yet: their `hgs` ends
 * and they answer "superseded". Outside the app, and in a build without
 * archive reads, there is nothing to cancel.
 */
export async function cancelArchivePages(through: number): Promise<void> {
  if (!inTauri()) return;
  await invoke<null>("cancel_archive_pages", { through }).catch(() => {});
}

/** Puts text on the clipboard: from Rust in the app, the page's clipboard in a browser. */
export async function copyText(text: string): Promise<void> {
  if (inTauri()) return invoke<void>("copy_text", { text });
  if (!text.trim()) throw "nothing to copy: the command is empty";
  if (/[\r\n]/.test(text)) throw "the command has a newline in it -- copying that is dangerous: the shell would run it the moment you paste";
  if (!navigator.clipboard) throw "The clipboard is unavailable.";
  await navigator.clipboard.writeText(text);
}

// Project organization edits (client/src-tauri `organization.rs`). Each is
// one call, which the Rust side sends as one `hgs swarm apply`; members are
// "<host>\n<session name>". Outside the app an edit is refused.

const DESKTOP_EDIT = "Project changes need the desktop app.";
const refusedOutside = (): EditOutcome => ({ outcome: "refused", detail: DESKTOP_EDIT });

/** Move to project: every member in one apply; `before` places them before that member (a drop). */
export async function moveToProject(members: string[], project: string, before: string | null): Promise<EditOutcome> {
  if (!inTauri()) return refusedOutside();
  return invoke<EditOutcome>("move_to_project", { members, project, before });
}

/** New project…, with the sessions it takes. */
export async function createProject(name: string, members: string[]): Promise<EditOutcome> {
  if (!inTauri()) return refusedOutside();
  return invoke<EditOutcome>("create_project", { name, members });
}

export async function renameProject(project: string, name: string): Promise<EditOutcome> {
  if (!inTauri()) return refusedOutside();
  return invoke<EditOutcome>("rename_project", { project, name });
}

/** Delete project…: its members go to `destination`. */
export async function deleteProject(project: string, destination: string): Promise<EditOutcome> {
  if (!inTauri()) return refusedOutside();
  return invoke<EditOutcome>("delete_project", { project, destination });
}

/** Moves a project before another, or to the end. */
export async function moveProject(project: string, before: string | null): Promise<EditOutcome> {
  if (!inTauri()) return refusedOutside();
  return invoke<EditOutcome>("move_project", { project, before });
}

/** Sort sessions by name. */
export async function sortProject(project: string): Promise<EditOutcome> {
  if (!inTauri()) return refusedOutside();
  return invoke<EditOutcome>("sort_project", { project });
}

/** Expand or collapse a project: this device's layout, no `hgs`. */
export async function setProjectCollapsed(project: string, collapsed: boolean): Promise<void> {
  if (!inTauri()) return;
  await invoke<void>("set_project_collapsed", { project, collapsed });
}

/** What the browser build shows instead of the screen reads. */
const DESKTOP_ONLY = "Reads need the desktop app.";

/** Tells the screens hub which screen shows; its reads run only while it does. */
export async function screenVisible(screen: Screen, visible: boolean): Promise<void> {
  if (!inTauri()) return;
  await invoke("screen_visible", { screen, visible }).catch((error) => console.warn("Zerus Next: screen_visible failed", error));
}

/** The Accounts and Overview account data; `force` is Refresh. */
export async function accountsView(force = false): Promise<AccountsView> {
  if (!inTauri()) return emptyAccounts([DESKTOP_ONLY]);
  return invoke<AccountsView>("accounts_view", { force });
}

/** The Machines view; `force` is Refresh. */
export async function machinesView(force = false): Promise<MachinesView> {
  if (!inTauri()) return emptyMachines(DESKTOP_ONLY);
  return invoke<MachinesView>("machines_view", { force });
}

/** Reads the selected peer's effective SSH connection. */
export async function machineResolve(alias: string): Promise<MachinesView> {
  if (!inTauri()) return emptyMachines(DESKTOP_ONLY);
  return invoke<MachinesView>("machine_resolve", { alias });
}

/** "Test connection": `machine check`, up to 25 s. */
export async function machineCheck(alias: string): Promise<MachinesView> {
  if (!inTauri()) return emptyMachines(DESKTOP_ONLY);
  return invoke<MachinesView>("machine_check", { alias });
}

export async function projectsView(): Promise<ProjectsView> {
  if (!inTauri()) return emptyProjects(DESKTOP_ONLY);
  return invoke<ProjectsView>("projects_view");
}

/** Calls `listener` with the screen whose view changed (`screens-changed`); returns the stop. */
export function onScreensChanged(listener: (screen: Screen) => void): () => void {
  if (!inTauri()) return () => {};
  let stop: (() => void) | undefined;
  let stopped = false;
  void listen<string>("screens-changed", (event) => listener(event.payload as Screen)).then((unlisten) => {
    if (stopped) unlisten();
    else stop = unlisten;
  });
  return () => {
    stopped = true;
    stop?.();
  };
}

const DESKTOP_TERMINAL = "Terminal needs the desktop app.";

export async function terminalConnect(view: string, target: TerminalTarget, cols: number, rows: number, onEvent: (m: ArrayBuffer | TerminalFinished) => void): Promise<TerminalConnected> {
  if (!inTauri()) throw DESKTOP_TERMINAL;
  const channel = new Channel<ArrayBuffer | TerminalFinished>();
  channel.onmessage = onEvent;
  return invoke<TerminalConnected>("terminal_connect", { view, target, cols, rows, on_event: channel });
}
export async function terminalWrite(view: string, generation: number, bytes: Uint8Array): Promise<void> {
  if (!inTauri()) return;
  await invoke("terminal_write", bytes, { headers: { "x-zerus-view": view, "x-zerus-generation": String(generation) } });
}
export async function terminalResize(view: string, generation: number, cols: number, rows: number): Promise<void> {
  if (inTauri()) await invoke("terminal_resize", { view, generation, cols, rows });
}
export function terminalAck(view: string, generation: number): void {
  if (inTauri()) void invoke("terminal_ack", { view, generation }).catch(() => {});
}
export async function terminalDisconnect(view: string): Promise<string | null> {
  return inTauri() ? invoke<string | null>("terminal_disconnect", { view }) : null;
}
export async function terminalRetarget(view: string, target: TerminalTarget): Promise<boolean> {
  return inTauri() ? invoke<boolean>("terminal_retarget", { view, target }) : true;
}
export async function terminalStatus(input: { facts: TerminalFacts; details: unknown; state: string; online: boolean; restored_pending: boolean; name: string; machine: string }): Promise<TerminalStatus> {
  if (!inTauri()) return { supported: false, available: false, reason: DESKTOP_TERMINAL, idle: "", connecting: "", disconnected: "" };
  return invoke<TerminalStatus>("terminal_status", { input });
}

/**
 * Whether the app window is shown, from src-tauri's `window-visibility` event
 * (shown and not minimized, as Qt's hideEvent sees it); the webview's own
 * visibility outside the app.
 */
let windowShown = true;
const windowListeners = new Set<() => void>();
let windowListening = false;
export const windowVisibility: PageVisibility = {
  hidden: () => (inTauri() ? !windowShown : documentVisibility.hidden()),
  onChange(listener) {
    if (!inTauri()) return documentVisibility.onChange(listener);
    windowListeners.add(listener);
    if (!windowListening) {
      windowListening = true;
      void listen<{ visible: boolean }>("window-visibility", (event) => {
        windowShown = !!event.payload.visible;
        for (const l of [...windowListeners]) l();
      });
    }
    return () => { windowListeners.delete(listener); };
  },
};

/** A session the menu bar or a notification opens: its computer and catalog key. */
/** `attention`: a clicked notification's session, shown on its Activity tab (Qt's `showAttentionSession`). */
export type OpenSession = { computer: ComputerId; key: string; attention?: boolean };
/** A notice for the window. */
export type DeskNotice = { title: string; text: string };
export type DeskPending = { open: OpenSession | null; notices: DeskNotice[] };

/** Listens to `name`; resolves with the stop once the listener is registered. */
function onEvent<T>(name: string, listener: (payload: T) => void): Promise<() => void> {
  if (!inTauri()) return Promise.resolve(() => {});
  return listen<T>(name, (event) => listener(event.payload));
}

/** `open-session`: the menu bar or a notification opens a session; resolves with the stop once listening. */
export const onOpenSession = (listener: (target: OpenSession) => void) => onEvent<OpenSession>("open-session", listener);
/** `desk-notice`: a notice for the window; resolves with the stop once listening. */
export const onDeskNotice = (listener: (notice: DeskNotice) => void) => onEvent<DeskNotice>("desk-notice", listener);

type DeskWindow = { __zerusDeskPage?: number; __zerusDeskPageReady?: (page: number) => void };

/** This page load's number, which the native side tells the page once it has loaded (`desk::page_loaded`). */
export function deskPage(): Promise<number> {
  if (!inTauri()) return Promise.resolve(0);
  const w = window as unknown as DeskWindow;
  if (typeof w.__zerusDeskPage === "number") return Promise.resolve(w.__zerusDeskPage);
  return new Promise((resolve) => { w.__zerusDeskPageReady = resolve; });
}

/** What the menu bar or a notification asked for before this page listened; `page` is `deskPage()`. */
export async function deskTakePending(page: number): Promise<DeskPending> {
  if (!inTauri()) return { open: null, notices: [] };
  return invoke<DeskPending>("desk_take_pending", { page });
}

// The composer (src-tauri/src/composer). Outside the app each call rejects
// with DESKTOP_ACTIONS, and there is no composer to open.

const DESKTOP_ACTIONS = "Actions need the desktop app.";
const desktop = <T>(command: string, args: Record<string, unknown>): Promise<T> =>
  inTauri() ? invoke<T>(command, args) : Promise.reject(DESKTOP_ACTIONS);

/** Opens the selected session's composer (undefined: none is open); null outside the app. */
export const composerOpen = (session: SessionRef | undefined) =>
  inTauri() ? invoke<ComposerView | null>("composer_open", { session: session ?? null }) : Promise.resolve(null);
export const composerEdit = (handle: number, rev: number, epoch: number, text: string, position: number, anchor: number) =>
  desktop<void>("composer_edit", { handle, rev, epoch, text, position, anchor });
/** The native file picker, then the reads; resolves once they ended. */
export const composerPick = (handle: number, start: number, end: number) => desktop<void>("composer_pick", { handle, start, end });
export const composerAttach = (handle: number, paths: string[], start: number, end: number) => desktop<void>("composer_attach", { handle, paths, start, end });
/** A pasted image goes as a raw IPC body, never as JSON. */
export async function composerPasteImage(handle: number, png: Uint8Array, start: number, end: number): Promise<void> {
  if (!inTauri()) throw DESKTOP_ACTIONS;
  await invoke("composer_paste_image", png, { headers: { "x-zerus-composer": String(handle), "x-zerus-start": String(start), "x-zerus-end": String(end) } });
}
export const composerRemove = (handle: number, index: number) => desktop<void>("composer_remove", { handle, index });
export const composerUndoRemove = (handle: number) => desktop<void>("composer_undo_remove", { handle });
export const composerAllowRetry = (handle: number) => desktop<void>("composer_allow_retry", { handle });
/** The status line's Refresh: the open session is read again (Qt's sessionRefreshRequested). */
export const composerRefresh = (handle: number) => desktop<void>("composer_refresh", { handle });
/** The detail tab that shows: Terminal and Native UI retry a failed read at Qt's background interval. */
export const composerTab = (tab: ComposerTab) => (inTauri() ? invoke<void>("composer_tab", { tab }) : Promise.resolve());
export const sendMessage = (handle: number, rev: number, epoch: number, text: string, position: number, anchor: number) =>
  desktop<SendReply>("send_message", { handle, rev, epoch, text, position, anchor });
export const sendAnswer = (token: number, answer: Answer) => desktop<SendReply>("send_answer", { token, answer });
export const sendCancel = (handle: number) => desktop<void>("send_cancel", { handle });
export const sendInterrupted = (handle: number) => desktop<void>("send_interrupted", { handle });
/** Answers once the clear ended (up to 45 s). */
export const clearContext = (handle: number, run: string, conversation: string, dismiss: boolean) =>
  desktop<SendReply>("clear_context", { handle, run, conversation, dismiss });
export const localRetry = (handle: number, id: string) => desktop<SendReply>("local_retry", { handle, id });
export const localDelete = (handle: number, id: string) => desktop<void>("local_delete", { handle, id });
export const localInspect = (handle: number, id: string) => desktop<Inspect | null>("local_inspect", { handle, id });
/** Reads an Activity attachment into the private cache, after the reads ahead of it (composer/fetch.rs). */
export async function attachmentFetch(computer: string, file: AttachmentRef): Promise<Fetched> {
  if (!inTauri()) throw "Attachments need the desktop app.";
  return invoke<Fetched>("attachment_fetch", { computer, file });
}

/** A view shows the cached copy at `path`: the cache bound keeps it until it is unpinned. */
export function attachmentPin(path: string) {
  if (inTauri()) void invoke("attachment_pin", { path }).catch(() => {});
}

export function attachmentUnpin(path: string) {
  if (inTauri()) void invoke("attachment_unpin", { path }).catch(() => {});
}

/** Opens an Activity attachment: null when it opened, a notice when the file manager shows it instead; rejects with why not. */
export async function attachmentOpen(computer: string, file: AttachmentRef): Promise<string | null> {
  if (!inTauri()) throw "Opening attachments needs the desktop app.";
  return invoke<string | null>("attachment_open", { computer, file });
}

/** Opens the composer attachment the view lists at `path` (`draft/{handle}/{list}/{index}`), as `attachmentOpen` answers. */
export async function attachmentOpenDraft(path: string): Promise<string | null> {
  if (!inTauri()) throw "Opening attachments needs the desktop app.";
  return invoke<string | null>("attachment_open_draft", { path });
}
