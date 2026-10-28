import { beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot, createSignal, mapArray } from "solid-js";
import type { Folder } from "../../types";
import type { AccountRow, ComputerRow } from "./newsession-ui";

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
  it("is Qt's binding under its id", () => {
    expect(view.SESSION_NEW).toEqual({ id: "session.new", group: "Sessions", label: "New session", keys: ["Ctrl+N"], context: "window" });
  });

  it("is Ctrl+N, and Command+N on macOS", () => {
    expect(view.isSessionNew(key({ ctrlKey: true }), false)).toBe(true);
    expect(view.isSessionNew(key({ ctrlKey: true, key: "N", shiftKey: true }), false)).toBe(false);
    expect(view.isSessionNew(key({ ctrlKey: true, altKey: true }), false)).toBe(false);
    expect(view.isSessionNew(key({ metaKey: true }), false)).toBe(false);
    expect(view.isSessionNew(key({ ctrlKey: true, metaKey: true }), false)).toBe(false);
    expect(view.isSessionNew(key({ ctrlKey: true, key: "m", code: "KeyM" }), false)).toBe(false);
    // Another layout's letter on the N key matches, as Qt falls back to the Latin key; a Latin letter there does not.
    expect(view.isSessionNew(key({ ctrlKey: true, key: "т", code: "KeyN" }), false)).toBe(true);
    expect(view.isSessionNew(key({ ctrlKey: true, key: "b", code: "KeyN" }), false)).toBe(false);
    // Qt's portable "Ctrl" is Command on macOS; physical Control is Meta there.
    expect(view.isSessionNew(key({ metaKey: true }), true)).toBe(true);
    expect(view.isSessionNew(key({ ctrlKey: true }), true)).toBe(false);
  });

  it("opens the dialog on every page, with the selected computer filter", () => {
    for (const page of ["dashboard", "sessions", "projects", "machines", "accounts", "settings"]) {
      const event = key({ ctrlKey: true });
      expect(view.windowKey(event, { mac: false, modal: false }), page).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
    }
    expect(store.openNewSession.mock.calls).toEqual(Array(6).fill([{ computer: "mac" }]));
  });

  it("does nothing under another modal dialog, or for another key", () => {
    expect(view.windowKey(key({ ctrlKey: true }), { mac: false, modal: true })).toBe(false);
    const other = key({ ctrlKey: true, key: "1", code: "Digit1" });
    expect(view.windowKey(other, { mac: false, modal: false })).toBe(false);
    expect(other.preventDefault).not.toHaveBeenCalled();
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
  it("has Qt's title and buttons, with Cancel the default and focused", () => {
    expect(view.WARNING_TITLE).toBe("Launch in another Windows session?");
    expect(view.WARNING_BUTTONS).toEqual([
      { label: "Launch anyway", answer: true, initial: false },
      { label: "Cancel", answer: false, initial: true },
    ]);
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
