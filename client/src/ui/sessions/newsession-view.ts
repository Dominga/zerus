import { createSignal } from "solid-js";
import { openFolderSession, openNewSession, selectedHostTarget } from "../../store/newSession";
import { notify } from "../../store/actions";
import type { AccountRow, ComputerRow, FolderRow, LaunchFolder, ProjectRow } from "./newsession-ui";

// The New session dialog's entry points and the logic its view keeps
// (NewSessionDialog.tsx renders it): every place Qt opens the dialog, the
// `session.new` shortcut, the Projects entries' texts, the local
// Windows-session warning, the dialog's keys and the keys that keep list
// rows in place. The dialog's rules stay in the store and the core.

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
 * `session.new` (Shortcuts.cpp:29, 58), a Window command of the shortcut
 * router (store/shortcuts.ts) in Settings > Keyboard's Sessions group, Ctrl+N
 * by default: App.tsx registers this for it, so it opens New session from any
 * page (SessionsWindowShortcuts.cpp:222). The router gives no command to a
 * modal dialog, as a Qt modal dialog blocks the window's shortcuts.
 */
export const newSessionFromShortcut = () => { void openNewSession({ computer: selectedHostTarget() }); };

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

// The dialog's keys (Qt's QDialog with QDialogButtonBox): Return and Enter
// press the default button, Start, when it is enabled. A closed non-editable
// QComboBox ignores them and opens on Alt+Down, F4 or Space; a line edit, a
// check box and a button Qt made `setAutoDefault(false)` pass them on too,
// and Space presses such a button. Cancel and Open Machines stay
// auto-default: Enter presses the one that has the focus.

/** A control of the dialog as Qt's key handling sees it. */
export type KeyControl =
  /** A project, computer or folder choice (a non-editable QComboBox). */
  | "choice"
  /** A button Qt made `setAutoDefault(false)`, or a QToolButton (Manage this project's folders). */
  | "button"
  /** A line edit. */
  | "text"
  /** A check box. */
  | "check"
  /** The Effort list (a native select, a non-editable QComboBox in Qt). */
  | "select";

/**
 * What a key does on a control:
 * - "start": Start, as the dialog's default button (the form's submit checks Start is enabled);
 * - "open": open the closed choice's list;
 * - "previous", "next", "first", "last": change the choice, or move in its open list;
 * - "pick": the open list's highlighted row;
 * - "close": close the open list, and let the key go on (Tab);
 * - "ignore": nothing (Qt's dialog takes Enter only without modifiers);
 * - "native": the control's own handling, such as Space pressing a button.
 */
export type KeyAction = "start" | "open" | "previous" | "next" | "first" | "last" | "pick" | "close" | "ignore" | "native";

type KeyPress = Pick<KeyboardEvent, "key" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey">;

/** The key mapping of the dialog's controls; `open` is whether the choice's list is open. */
export function dialogKey(control: KeyControl, e: KeyPress, open = false): KeyAction {
  const plain = !e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey;
  if (control === "choice" && open) {
    if (e.key === "ArrowUp") return "previous";
    if (e.key === "ArrowDown") return "next";
    if (e.key === "Home") return "first";
    if (e.key === "End") return "last";
    if (e.key === "Enter" || e.key === " ") return "pick";
    if (e.key === "Tab") return "close";
    return "native";
  }
  if (e.key === "Enter") return plain ? "start" : "ignore";
  if (control !== "choice") return "native";
  // QComboBox::keyPressEvent on a closed, non-editable combo box; Control
  // (Command on macOS) with an arrow is left to the window.
  const command = e.ctrlKey || e.metaKey;
  if ((e.key === "ArrowDown" && e.altKey) || (e.key === "F4" && plain) || e.key === " ") return "open";
  if ((e.key === "ArrowUp" && !command) || e.key === "PageUp") return "previous";
  if ((e.key === "ArrowDown" && !command) || e.key === "PageDown") return "next";
  if (e.key === "Home") return "first";
  if (e.key === "End") return "last";
  return "native";
}
