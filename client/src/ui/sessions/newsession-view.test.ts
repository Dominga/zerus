import { beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot, createSignal, mapArray } from "solid-js";
import type { Folder } from "../../types";
import type { AccountRow, ComputerRow } from "./newsession-ui";
import { command, handlerFor, onCommand, portableDefaults, route, sequenceState, type Platform } from "../../store/shortcuts";

// The New session dialog's entry points and its view logic
// (tray/src/SessionsWindow.cpp 295-297, 377-378, 2572-2573;
// SessionsWindowShortcuts.cpp:222 with Shortcuts.cpp:29, 58;
// ProjectsDialog.cpp 180, 515-560; LocalLaunchWarning.h 18-28). The store is
// mocked: each entry is checked by the call it makes. Synthetic names only.

const store = vi.hoisted(() => ({
  host: "mac",
  folderProblem: "",
  openNewSession: vi.fn(async (_options?: unknown) => {}),
  openFolderSession: vi.fn(async (_project: string, _folder: unknown) => ""),
}));
const notices = vi.hoisted(() => [] as { tone: string; text: string }[]);

vi.mock("../../store/newSession", () => ({
  openNewSession: store.openNewSession,
  openFolderSession: async (project: string, folder: unknown) => { await store.openFolderSession(project, folder); return store.folderProblem; },
  selectedHostTarget: () => store.host,
}));
vi.mock("../../store/actions", () => ({ notify: (tone: string, text: string) => notices.push({ tone, text }) }));

const view = await import("./newsession-view");

const key = (init: Partial<KeyboardEvent>) => ({ key: "n", code: "KeyN", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, preventDefault: vi.fn(), ...init });
const folder = (id: string, machine: string, path: string): Folder => ({ id, key: id, name: path.split("/").pop() ?? path, path, computer: machine, machine });

beforeEach(() => {
  store.host = "mac";
  store.folderProblem = "";
  store.openNewSession.mockClear();
  store.openFolderSession.mockClear();
  notices.length = 0;
});

describe("the entry points (each Qt's showNewSession call)", () => {
  it("opens from the rail and the Sessions header with the selected computer filter", () => {
    view.newSessionFromRail();
    view.newSessionFromPanel();
    expect(store.openNewSession.mock.calls).toEqual([[{ computer: "mac" }], [{ computer: "mac" }]]);
  });

  it("opens from a project header's menu for that project", () => {
    store.host = "";
    view.newSessionForProject("Work");
    expect(store.openNewSession.mock.calls).toEqual([[{ project: "Work", computer: "" }]]);
  });

  it("opens a Projects folder through openFolderSession and shows why it could not", async () => {
    const f = folder("f1", "pc", "/srv/a");
    await view.newSessionInFolder("work", f);
    expect(store.openFolderSession.mock.calls).toEqual([["work", f]]);
    expect(notices).toEqual([]);
    store.folderProblem = "This project folder changed. Select it again.";
    await view.newSessionInFolder("work", f);
    expect(notices).toEqual([{ tone: "warn", text: "This project folder changed. Select it again." }]);
    expect(store.openNewSession).not.toHaveBeenCalled();
  });
});

describe("session.new (Shortcuts.cpp:29, 58, a Window shortcut)", () => {
  // The shortcut router (store/shortcuts.ts) owns the binding; App.tsx registers newSessionFromShortcut for it.
  const runs = (init: Partial<KeyboardEvent>, on: Platform = "linux") =>
    route(key(init) as unknown as KeyboardEvent, sequenceState(), on).run === "session.new";

  it("is Qt's binding under its id", () => {
    expect(command("session.new")).toEqual({ id: "session.new", group: "Sessions", title: "New session", scope: "window" });
    expect(portableDefaults("session.new", "linux")).toEqual(["Ctrl+N"]);
    expect(portableDefaults("session.new", "macos")).toEqual(["Ctrl+N"]);
  });

  it("is Ctrl+N, and Command+N on macOS", () => {
    const off = onCommand("session.new", view.newSessionFromShortcut);
    expect(runs({ ctrlKey: true })).toBe(true);
    expect(runs({ ctrlKey: true, key: "N", shiftKey: true })).toBe(false);
    expect(runs({ ctrlKey: true, altKey: true })).toBe(false);
    expect(runs({ metaKey: true })).toBe(false);
    expect(runs({ ctrlKey: true, metaKey: true })).toBe(false);
    expect(runs({ ctrlKey: true, key: "m", code: "KeyM" })).toBe(false);
    // Another layout's letter on the N key matches, as Qt falls back to the Latin key; a Latin letter there does not.
    expect(runs({ ctrlKey: true, key: "т", code: "KeyN" })).toBe(true);
    expect(runs({ ctrlKey: true, key: "b", code: "KeyN" })).toBe(false);
    // Qt's portable "Ctrl" is Command on macOS; physical Control is Meta there.
    expect(runs({ metaKey: true }, "macos")).toBe(true);
    expect(runs({ ctrlKey: true }, "macos")).toBe(false);
    off();
    expect(store.openNewSession).not.toHaveBeenCalled();
  });

  it("opens the dialog with the selected computer filter", () => {
    const off = onCommand("session.new", view.newSessionFromShortcut);
    handlerFor("session.new", null)?.();
    off();
    expect(store.openNewSession.mock.calls).toEqual([[{ computer: "mac" }]]);
  });

  it("does nothing under another modal dialog, or for another key", () => {
    const off = onCommand("session.new", view.newSessionFromShortcut);
    const inDialog = { closest: (selector: string) => (selector === "dialog[open]" ? { getAttribute: () => null } : null), matches: () => false };
    expect(runs({ ctrlKey: true, target: inDialog as unknown as EventTarget })).toBe(false);
    expect(runs({ ctrlKey: true, key: "1", code: "Digit1" })).toBe(false);
    off();
    expect(store.openNewSession).not.toHaveBeenCalled();
  });
});

describe("the Projects entries (ProjectsDialog.cpp 520-522, 556-558)", () => {
  it("are off with Qt's problem as their hint, else show the folder", () => {
    expect(view.folderMenuEntry({ problem: "", shown: "/srv/a" })).toEqual({ disabled: false, hint: "/srv/a" });
    expect(view.folderMenuEntry({ problem: "Reconnect mac to start a session in this folder.", shown: "/srv/a" }))
      .toEqual({ disabled: true, hint: "Reconnect mac to start a session in this folder." });
    expect(view.folderButton({ problem: "", shown: "C:\\src\\app" }, "studio")).toEqual({ disabled: false, tip: "New session in C:\\src\\app on studio" });
    expect(view.folderButton({ problem: "Select a folder for the new session.", shown: "" }, "")).toEqual({ disabled: true, tip: "Select a folder for the new session." });
  });
});

describe("the local Windows-session warning (LocalLaunchWarning.h 18-28)", () => {
  // The warning renders its buttons from WARNING_BUTTONS; Cancel is Qt's default and escape button (27-28).
  it("answers each button as Qt does, and the focused default answers as Escape", async () => {
    await createRoot(async (dispose) => {
      const confirm = view.createConfirm();
      const answers: boolean[] = [];
      for (const button of view.WARNING_BUTTONS) {
        const asked = confirm.ask("text");
        confirm.answer(button.answer);
        answers.push(await asked);
      }
      expect(answers).toEqual([true, false]);
      const escaped = confirm.ask("text");
      confirm.cancel();
      const initial = view.WARNING_BUTTONS.filter((b) => b.initial);
      expect(initial.length).toBe(1);
      expect(initial[0].answer).toBe(await escaped);
      dispose();
    });
  });

  it("answers once: Launch anyway is true, Escape and closing are Cancel", async () => {
    await createRoot(async (dispose) => {
      const confirm = view.createConfirm();
      expect(confirm.text()).toBeUndefined();
      const first = confirm.ask("text one");
      expect(confirm.text()).toBe("text one");
      confirm.cancel();
      expect(await first).toBe(false);
      expect(confirm.text()).toBeUndefined();
      const second = confirm.ask("text two");
      confirm.answer(true);
      confirm.answer(false);
      expect(await second).toBe(true);
      // A new question while one is open cancels the open one.
      const stale = confirm.ask("text three");
      const fresh = confirm.ask("text four");
      expect(await stale).toBe(false);
      expect(confirm.text()).toBe("text four");
      confirm.cancel();
      expect(await fresh).toBe(false);
      dispose();
    });
  });
});

describe("row keys (NewSessionDialog.cpp 503-509, 908-911)", () => {
  // An open list keeps its hovered row while fleet polls and limits relabel it in place.
  it("keep each computer and account row across relabels, and only those", () => {
    createRoot((dispose) => {
      const [computers, setComputers] = createSignal<ComputerRow[]>([{ label: "pc", target: "", online: true }, { label: "mac", target: "mac", online: true }]);
      const nodes = mapArray(() => computers().map(view.computerKey), (k) => ({ k }));
      const before = nodes();
      setComputers([{ label: "pc", target: "", online: true }, { label: "mac (offline)", target: "mac", online: false }]);
      expect(nodes()).toEqual(before);
      expect(nodes()[1]).toBe(before[1]);
      setComputers([{ label: "pc", target: "", online: true }, { label: "lab", target: "lab", online: true }]);
      expect(nodes()[0]).toBe(before[0]);
      expect(nodes()[1]).not.toBe(before[1]);

      const row = (k: string, name: string) => ({ key: k, name } as AccountRow);
      const [accounts, setAccounts] = createSignal([row("codex\nwork", "a@example.invalid"), row("sh\n", "Run shell")]);
      const accountNodes = mapArray(() => accounts().map(view.accountKey), (k) => ({ k }));
      const shown = accountNodes();
      setAccounts([row("codex\nwork", "Account n/a"), row("sh\n", "Run shell")]);
      expect(accountNodes()[0]).toBe(shown[0]);
      expect(accountNodes()[1]).toBe(shown[1]);
      dispose();
    });
  });
});

describe("the dialog's keys (QDialog default button, QComboBox::keyPressEvent)", () => {
  const press = (keyName: string, extra: Partial<KeyboardEvent> = {}) => ({ key: keyName, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...extra });

  it("gives Enter to Start from a closed choice, a line edit, a check box, the Effort list and the non-default buttons", () => {
    for (const control of ["choice", "button", "text", "check", "select"] as const) {
      expect(view.dialogKey(control, press("Enter")), control).toBe("start");
      // Qt's dialog takes Return and Enter only without modifiers.
      expect(view.dialogKey(control, press("Enter", { shiftKey: true })), control).toBe("ignore");
      expect(view.dialogKey(control, press("Enter", { ctrlKey: true })), control).toBe("ignore");
    }
  });

  it("leaves Space to the buttons and the check box, and typing to the fields", () => {
    expect(view.dialogKey("button", press(" "))).toBe("native");
    expect(view.dialogKey("check", press(" "))).toBe("native");
    expect(view.dialogKey("text", press("a"))).toBe("native");
    expect(view.dialogKey("select", press("ArrowDown"))).toBe("native");
    expect(view.dialogKey("button", press("Escape"))).toBe("native");
  });

  it("opens a closed choice on Alt+Down, F4 and Space, never on Enter", () => {
    expect(view.dialogKey("choice", press("ArrowDown", { altKey: true }))).toBe("open");
    expect(view.dialogKey("choice", press("F4"))).toBe("open");
    expect(view.dialogKey("choice", press("F4", { shiftKey: true }))).toBe("native");
    expect(view.dialogKey("choice", press(" "))).toBe("open");
    expect(view.dialogKey("choice", press("Enter"))).toBe("start");
  });

  it("changes a closed choice with the arrows, PageUp, PageDown, Home and End", () => {
    expect(view.dialogKey("choice", press("ArrowUp"))).toBe("previous");
    expect(view.dialogKey("choice", press("PageUp"))).toBe("previous");
    expect(view.dialogKey("choice", press("ArrowDown"))).toBe("next");
    expect(view.dialogKey("choice", press("PageDown"))).toBe("next");
    expect(view.dialogKey("choice", press("Home"))).toBe("first");
    expect(view.dialogKey("choice", press("End"))).toBe("last");
    expect(view.dialogKey("choice", press("ArrowDown", { ctrlKey: true }))).toBe("native");
    expect(view.dialogKey("choice", press("ArrowUp", { metaKey: true }))).toBe("native");
    // Escape is the dialog's: it closes an open list first, else the dialog.
    expect(view.dialogKey("choice", press("Escape"))).toBe("native");
  });

  it("moves in an open list and picks its row on Enter or Space", () => {
    expect(view.dialogKey("choice", press("ArrowDown"), true)).toBe("next");
    expect(view.dialogKey("choice", press("ArrowUp"), true)).toBe("previous");
    expect(view.dialogKey("choice", press("Home"), true)).toBe("first");
    expect(view.dialogKey("choice", press("End"), true)).toBe("last");
    expect(view.dialogKey("choice", press("Enter"), true)).toBe("pick");
    expect(view.dialogKey("choice", press(" "), true)).toBe("pick");
    expect(view.dialogKey("choice", press("Tab"), true)).toBe("close");
    expect(view.dialogKey("choice", press("Escape"), true)).toBe("native");
  });
});
