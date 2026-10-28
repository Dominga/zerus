import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { emptyAccounts, emptyBatch, emptyMachines, emptyProjects, isEmptyBatch, type AccountsView, type MachinesView, type ProjectsView, type Screen, type Action, type Batch, type ComputerId, type Details, type Outcome, type ReadMark, type Target } from "./types";
import type { LaunchAccounts, LaunchChoices, LaunchPlan, LaunchStart, MachineMeta, NewSessionContext, NewSessionFormInput, NewSessionFormView, PathStyle, RepositoryFields } from "./types";
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

/** Missing arrays default to empty, so a core that omits empty fields still applies. */
function normalize(b: Partial<Batch>): Batch {
  const e = emptyBatch();
  return { upserts: b.upserts ?? e.upserts, removes: b.removes ?? e.removes, replaces: b.replaces ?? e.replaces, computers: b.computers ?? e.computers, machines: b.machines ?? [], activity: b.activity ?? e.activity };
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

/** The inspect fields of a session; null when they are unavailable. */
export async function inspectSession(target: Target, archiveId: string | null): Promise<Details | null> {
  if (!inTauri()) return null;
  return (await invoke<Details | null>("inspect_session", { target, archiveId })) ?? null;
}

/** Read marks whose delivery has not been confirmed yet. */
export async function pendingReadMarks(): Promise<ReadMark[]> {
  if (!inTauri()) return [];
  return invoke<ReadMark[]>("pending_read_marks");
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

/** Puts text on the clipboard: from Rust in the app, the page's clipboard in a browser. */
export async function copyText(text: string): Promise<void> {
  if (inTauri()) return invoke<void>("copy_text", { text });
  if (!text.trim()) throw "nothing to copy: the command is empty";
  if (/[\r\n]/.test(text)) throw "the command has a newline in it -- copying that is dangerous: the shell would run it the moment you paste";
  if (!navigator.clipboard) throw "The clipboard is unavailable.";
  await navigator.clipboard.writeText(text);
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

// New session (client/src-tauri `newsession.rs`, and `screens.rs` for the
// accounts). In the browser build every command answers a failure.

const NEW_SESSION_DESKTOP_ONLY = "New session needs the desktop app.";

async function desktop<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri()) throw NEW_SESSION_DESKTOP_ONLY;
  return invoke<T>(command, args);
}

/** The generated branch, this process's Windows session and the remembered choices. */
export const newSessionContext = () => desktop<NewSessionContext>("new_session_context");
/** The form's view: the core's `launch::shown` with Auto, its path and the project worktree derived. */
export const newSessionForm = (input: NewSessionFormInput) => desktop<NewSessionFormView>("new_session_form", { input });
/** What Start does now; asked again at the click. */
export const newSessionStart = (input: NewSessionFormInput, submitted: boolean) => desktop<LaunchStart>("new_session_start", { input, submitted });
/** The plan at the click: the form's fields and what the core derives from them come from `input`, as Start reads them. */
export const newSessionPlan = (input: NewSessionFormInput, choices: LaunchChoices) => desktop<LaunchPlan>("new_session_plan", { input, choices });
/**
 * Background creation's admission (Task 5's `creation_submit`): the admitted creation's id; a rejection is the
 * refusal's text, which the dialog shows. Never retried.
 */
export const creationSubmit = (plan: LaunchPlan) => desktop<string>("creation_submit", { plan });
/** The Windows-session warning's text when Start must ask first; `computer` is "" for this one. */
export const launchWarning = (local: MachineMeta, computer: ComputerId) => desktop<string | null>("launch_warning", { local, computer });
/** The repository fields; `fresh` (the old record's folder name, kept even when empty) starts them anew on another computer or style. */
export const repositoryFields = (url: string, destination: string, style: PathStyle, custom: boolean, fresh: string | null = null) =>
  desktop<RepositoryFields>("repository_fields", { url, destination, style, custom, fresh });
/** The folder's saved catalog, `{}` without one. */
export const worktreeSnapshot = (computer: ComputerId, path: string) => desktop<Record<string, unknown>>("worktree_snapshot", { computer, path });
/** The folder's catalog: a fresh snapshot, or `hgs worktrees --path`; a rejection is its error. */
export const worktreeCatalog = (computer: ComputerId, path: string, refresh: boolean) => desktop<Record<string, unknown>>("worktree_catalog", { computer, path, refresh });
/** Create worktree: the new checkout's path; a rejection is Qt's text. Never retried. */
export const createWorktree = (computer: ComputerId, path: string, commonDir: string, branch: string, base: string, destination: string) =>
  desktop<string>("create_worktree", { computer, path, commonDir, branch, base, destination });
export const listDirectories = (computer: ComputerId, path: string, hidden: boolean) => desktop<Record<string, unknown>>("list_directories", { computer, path, hidden });
/** New folder in the folder browser: its receipt. A lost one is read again, never sent again. */
export const createDirectory = (computer: ComputerId, path: string) => desktop<Record<string, unknown>>("create_directory", { computer, path });
/** The system folder dialog on this computer; null when cancelled. */
export const pickLocalFolder = (start: string) => desktop<string | null>("pick_local_folder", { start });
export const localFolderExists = (path: string) => desktop<boolean>("local_folder_exists", { path });
/** The folder's OpenCode model catalog; `name` words an older computer's update text. */
export const openCodeModels = (computer: ComputerId, folder: string, name: string) => desktop<Record<string, unknown>>("opencode_models", { computer, folder, name });
/** One star: one request identity, never sent again. */
export const openCodeFavorite = (computer: ComputerId, value: string, favorite: boolean, name: string) =>
  desktop<Record<string, unknown>>("opencode_favorite", { computer, value, favorite, name });
/** Start OpenCode on {computer}. */
export const openCodeStart = (computer: ComputerId) => desktop<Record<string, unknown>>("opencode_start", { computer });
/**
 * One computer's account rows, on the dialog's open, a computer change (`refresh` false) and Refresh: reads its catalog
 * (sharing a running read), then asks for limits. `agent` is the dialog's requested agent.
 */
export const launchAccounts = (computer: ComputerId, refresh: boolean, agent: string | null = null) =>
  desktop<LaunchAccounts>("launch_accounts", { computer, refresh, agent });
/** The rows again, reading nothing; the grouping stays the last catalog's. */
export const launchAccountRows = (computer: ComputerId) => desktop<LaunchAccounts>("launch_account_rows", { computer });
/** The dialog closed: its computer's replies stop sending `launch-accounts-changed`. */
export const launchAccountsClose = () => desktop<void>("launch_accounts_close");

/** Calls `listener` with the computer whose catalog or limits changed while a dialog reads it; returns the stop. */
export function onLaunchAccountsChanged(listener: (computer: ComputerId) => void): () => void {
  if (!inTauri()) return () => {};
  let stop: (() => void) | undefined;
  let stopped = false;
  void listen<string>("launch-accounts-changed", (event) => listener(event.payload)).then((unlisten) => {
    if (stopped) unlisten();
    else stop = unlisten;
  });
  return () => {
    stopped = true;
    stop?.();
  };
}
