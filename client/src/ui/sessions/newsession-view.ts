import { createSignal } from "solid-js";
import { openFolderSession, openNewSession, selectedHostTarget } from "../../store/newSession";
import { notify } from "../../store/actions";
import type { AccountRow, ComputerRow, FolderRow, LaunchFolder, ProjectRow } from "./newsession-ui";

// The New session dialog's entry points and the logic its view keeps
// (NewSessionDialog.tsx renders it): every place Qt opens the dialog, the
// `session.new` shortcut, the Projects entries' texts, the local
// Windows-session warning and the keys that keep list rows in place. The
// dialog's rules stay in the store and the core.

// Entry points: each is Qt's showNewSession call.

/** The rail's New session button (SessionsWindow.cpp 295-297). */
export const newSessionFromRail = () => { void openNewSession({ computer: selectedHostTarget() }); };
/** The Sessions header's New session button (377-378). */
export const newSessionFromPanel = () => { void openNewSession({ computer: selectedHostTarget() }); };
/** A project header's "New session…" (showGroupMenu 2572-2573); `project` is its id or name. */
export const newSessionForProject = (project: string) => { void openNewSession({ project, computer: selectedHostTarget() }); };

/**
 * Projects' "New session…" for a folder (ProjectsDialog::newFolderSession
 * 533-541). Qt shows why it could not open in the Projects status line; here
 * that text is a notice.
 */
export async function newSessionInFolder(project: string, folder: LaunchFolder): Promise<void> {
  const problem = await openFolderSession(project, folder);
  if (problem) notify("warn", problem);
}

/** What a Projects entry shows of `folderLaunch`. */
export interface FolderLaunch { problem: string; shown: string }

/** The folder menu's entry (556-558): off with the problem as its hint, else the folder as shown. */
export const folderMenuEntry = (state: FolderLaunch) => ({ disabled: !!state.problem, hint: state.problem || state.shown });

/** The Folders "New session…" button (520-522): its tooltip names the folder and its computer, or the problem. */
export const folderButton = (state: FolderLaunch, machine: string) =>
  ({ disabled: !!state.problem, tip: state.problem || `New session in ${state.shown} on ${machine}` });

// The shortcut.

/**
 * `session.new` (Shortcuts.cpp:29, 58): a Window shortcut in Settings >
 * Keyboard's Sessions group. Until the client has the shortcut registry, this
 * is its fixed default; the registry takes the id over as it is.
 */
export const SESSION_NEW = { id: "session.new", group: "Sessions", label: "New session", keys: ["Ctrl+N"], context: "window" } as const;

type Keys = Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">;

/**
 * Qt's portable "Ctrl+N": Command+N on macOS, Control+N elsewhere, with no
 * other modifier. On a layout without Latin letters the N key's own letter
 * matches too, as Qt falls back to the Latin key.
 */
export function isSessionNew(e: Keys, mac: boolean): boolean {
  const key = e.key.toLowerCase();
  const n = key === "n" || (e.code === "KeyN" && !/^[a-z]$/.test(key));
  if (!n || e.altKey || e.shiftKey) return false;
  return mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
}

const MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** Another modal dialog owns the keyboard, as a Qt modal dialog blocks the window's shortcuts. */
function modalOpen(): boolean {
  if (typeof document === "undefined") return false;
  try { return !!document.querySelector("dialog:modal"); } catch { return !!document.querySelector("dialog[open]"); }
}

/**
 * The window's key handler for `session.new` (SessionsWindowShortcuts.cpp:222):
 * it opens New session from any page. True when it took the key.
 */
export function windowKey(e: Keys & Pick<KeyboardEvent, "preventDefault">, at: { mac: boolean; modal: boolean } = { mac: MAC, modal: modalOpen() }): boolean {
  if (at.modal || !isSessionNew(e, at.mac)) return false;
  e.preventDefault();
  void openNewSession({ computer: selectedHostTarget() });
  return true;
}

// The local Windows-session warning (LocalLaunchWarning.h 18-28).

export const WARNING_TITLE = "Launch in another Windows session?";

/** Its buttons in order: Cancel is the default and has the focus; Escape is Cancel. */
export const WARNING_BUTTONS = [
  { label: "Launch anyway", answer: true, initial: false },
  { label: "Cancel", answer: false, initial: true },
] as const;

/**
 * The warning's question: Start asks it with the core's text and waits.
 * Launch anyway answers true; Cancel, Escape and closing answer false. Each
 * question is answered once, and a new one cancels the open one.
 */
export function createConfirm() {
  const [text, setText] = createSignal<string>();
  let resolve: ((answer: boolean) => void) | undefined;
  const answer = (value: boolean) => {
    const done = resolve;
    resolve = undefined;
    setText(undefined);
    done?.(value);
  };
  return {
    /** The open question's text. */
    text,
    ask(question: string): Promise<boolean> {
      answer(false);
      setText(question);
      return new Promise<boolean>((done) => { resolve = done; });
    },
    answer,
    /** Cancel, Escape or the dialog closing. */
    cancel: () => answer(false),
  };
}

// Row keys: an open list keeps its rows (and the hovered one) while fleet
// polls and limits relabel them in place (NewSessionDialog.cpp 503-509, 908-911).

export const computerKey = (row: ComputerRow) => row.target;
export const accountKey = (row: AccountRow) => row.key;
export const projectKey = (row: ProjectRow) => (row.newProject ? "\nnew" : row.id);
export const folderKey = (row: FolderRow) => `${row.key}\n${row.path}`;

