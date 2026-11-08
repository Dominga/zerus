// @vitest-environment happy-dom
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

const spies = vi.hoisted(() => ({ fleetRefresh: vi.fn(() => Promise.resolve()), rename: vi.fn(), renameDisabled: false }));
vi.mock("../../bridge", async (importOriginal) => ({ ...(await importOriginal<typeof import("../../bridge")>()), fleetRefresh: spies.fleetRefresh }));
vi.mock("./ActionDialogs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./ActionDialogs")>()),
  actionEntries: () => ({ rename: { label: "Rename…", disabled: spies.renameDisabled, action: spies.rename } }),
}));

import { applyBatch } from "../../store/catalog";
import { composer } from "../../store/composer";
import { machineEditor } from "../../store/machineEditor";
import { notices } from "../../store/notices";
import { installShortcutRouter, handlerFor } from "../../store/shortcuts";
import { setSearchCursor } from "../../store/contentSearch";
import { detailTab, page, selectedKey, setDetailTab, setPage, setSearch, setSelectedKey } from "../../store/ui";
import { rowOut } from "../../store/testing";
import { emptyBatch } from "../../types";
import { installWindowCommands } from "./window-commands";

// Every window command that had no handler, driven by its default Linux
// chord through the shortcut router, as a key press in the window reaches it.

const id = (i: number) => `\nrun:${i}`;
const rows = [rowOut("", 1), rowOut("", 2, { status: "attention" }), rowOut("", 3), rowOut("", 4, { status: "attention" })];

let undo: (() => void)[] = [];
beforeAll(() => applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: rows }));
afterAll(() => applyBatch({ ...emptyBatch(), removes: rows.map((row) => ({ computer: "", key: row.key })) }));
beforeEach(() => {
  setPage("sessions"); setSearch(""); setSearchCursor(""); setDetailTab("activity"); setSelectedKey(undefined);
  undo = [installShortcutRouter(window), installWindowCommands()];
});
afterEach(() => {
  undo.forEach((off) => off());
  document.body.replaceChildren();
  setSelectedKey(undefined); setDetailTab("activity");
  vi.clearAllMocks();
  spies.renameDisabled = false;
});

type Press = { key: string; code: string; ctrl?: boolean; alt?: boolean; shift?: boolean };
function press(p: Press, target: Element = document.body) {
  const event = new KeyboardEvent("keydown", { key: p.key, code: p.code, ctrlKey: !!p.ctrl, altKey: !!p.alt, shiftKey: !!p.shift, bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event.defaultPrevented;
}
const ALT_DOWN = { key: "ArrowDown", code: "ArrowDown", alt: true };
const ALT_UP = { key: "ArrowUp", code: "ArrowUp", alt: true };

/** The Sessions page's parts the commands look for, as SessionList and SessionDetail render them. */
function sessionsPage(tabs = ["activity", "terminal", "processes"]) {
  document.body.innerHTML = `
    <section class="sessions-pane"><div class="sp-panel"><div class="sp-search"><input value="older query"></div>
      <div class="sp-scroll" role="tree" tabindex="0"></div></div></section>
    <section class="detail"><div class="detail-tabs" role="tablist">
      ${tabs.map((tab) => `<button class="tab" role="tab" data-tab="${tab}" aria-selected="${tab === detailTab()}"></button>`).join("")}
    </div><div class="tab-body"><div class="act-scroll" tabindex="-1"></div>
      <div class="composer"><textarea class="cmp-editor"></textarea></div></div>
      <aside class="inspector"><div class="insp-tabs"><button class="tab" role="tab" aria-selected="true"></button></div></aside></section>`;
  // A tab click shows its view, as SessionDetail's tab buttons do.
  for (const tab of document.querySelectorAll<HTMLButtonElement>("[data-tab]")) {
    tab.onclick = () => {
      setDetailTab(tab.dataset.tab as never);
      for (const other of document.querySelectorAll("[data-tab]")) other.setAttribute("aria-selected", String(other === tab));
    };
  }
  const q = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
  return { list: q(".sp-scroll"), search: q<HTMLInputElement>(".sp-search input"), view: q(".act-scroll"), editor: q(".cmp-editor"), panel: q(".inspector .tab") };
}

describe("window commands from the keyboard", () => {
  it("Alt+Down selects the next session and stops at the end", () => {
    expect(press(ALT_DOWN)).toBe(true);
    expect(selectedKey()).toBe(id(1));
    setSelectedKey(id(3));
    press({ key: "PageDown", code: "PageDown", ctrl: true });
    expect(selectedKey()).toBe(id(4));
    press(ALT_DOWN);
    expect(selectedKey()).toBe(id(4));
  });

  it("Alt+Up selects the previous session, from another page as well", () => {
    setPage("projects");
    press(ALT_UP);
    expect(page()).toBe("sessions");
    expect(selectedKey()).toBe(id(4));
    press(ALT_UP);
    expect(selectedKey()).toBe(id(3));
  });

  it("Alt+Shift+Down goes to the next session that needs attention and wraps", () => {
    setSelectedKey(id(2));
    press({ ...ALT_DOWN, shift: true });
    expect(selectedKey()).toBe(id(4));
    press({ ...ALT_DOWN, shift: true });
    expect(selectedKey()).toBe(id(2));
  });

  it("Alt+Shift+Up goes to the previous session that needs attention, or says none does", () => {
    setSelectedKey(id(3));
    press({ ...ALT_UP, shift: true });
    expect(selectedKey()).toBe(id(2));
    const filtered = rows.map((row) => ({ ...row, row: { ...row.row, status: "ready" } }));
    applyBatch({ ...emptyBatch(), upserts: filtered });
    try {
      press({ ...ALT_UP, shift: true });
      expect(selectedKey()).toBe(id(2));
      expect(notices().some((notice) => notice.text === "No sessions need attention")).toBe(true);
    } finally {
      applyBatch({ ...emptyBatch(), upserts: rows });
    }
  });

  it("Alt+Down steps through search results while they are shown", () => {
    setSearch("t3");
    press(ALT_DOWN);
    expect(selectedKey()).toBe(id(3));
    press(ALT_DOWN);
    expect(selectedKey()).toBe(id(3));
  });

  it("F2 renames the selected session, and only when Rename is available", () => {
    press({ key: "F2", code: "F2" });
    expect(spies.rename).not.toHaveBeenCalled();
    setSelectedKey(id(1));
    press({ key: "F2", code: "F2" });
    expect(spies.rename).toHaveBeenCalledTimes(1);
    spies.renameDisabled = true;
    press({ key: "F2", code: "F2" });
    expect(spies.rename).toHaveBeenCalledTimes(1);
  });

  it("Ctrl+F focuses the session search and selects its text", () => {
    const { search } = sessionsPage();
    setPage("settings");
    press({ key: "f", code: "KeyF", ctrl: true });
    expect(page()).toBe("sessions");
    expect(document.activeElement).toBe(search);
    expect([search.selectionStart, search.selectionEnd]).toEqual([0, search.value.length]);
  });

  it("Ctrl+R and F5 refresh the fleet, the selected session and an open Machines page", () => {
    const refresh = vi.spyOn(composer, "refresh").mockResolvedValue();
    const choose = vi.spyOn(machineEditor, "choose").mockImplementation(() => {});
    press({ key: "r", code: "KeyR", ctrl: true });
    expect(spies.fleetRefresh).toHaveBeenCalledTimes(1);
    expect(refresh).not.toHaveBeenCalled();
    setSelectedKey(id(1)); setPage("machines");
    press({ key: "F5", code: "F5" });
    expect(spies.fleetRefresh).toHaveBeenCalledTimes(2);
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(choose).toHaveBeenCalledWith(machineEditor.selected(), true);
    refresh.mockRestore(); choose.mockRestore();
  });

  it("Ctrl+Tab shows the next tab and focuses its view, wrapping", () => {
    const { view } = sessionsPage();
    setSelectedKey(id(1));
    press({ key: "Tab", code: "Tab", ctrl: true });
    expect(detailTab()).toBe("terminal");
    press({ key: "Tab", code: "Tab", ctrl: true });
    press({ key: "Tab", code: "Tab", ctrl: true });
    expect(detailTab()).toBe("activity");
    expect(document.activeElement).toBe(view);
  });

  it("Ctrl+Shift+Tab shows the previous tab, wrapping", () => {
    sessionsPage();
    setSelectedKey(id(1));
    press({ key: "Tab", code: "Tab", ctrl: true, shift: true });
    expect(detailTab()).toBe("processes");
    press({ key: "Tab", code: "Tab", ctrl: true, shift: true });
    expect(detailTab()).toBe("terminal");
  });

  it("Ctrl+` switches to Terminal and back to Activity's message field", () => {
    const { editor } = sessionsPage();
    setSelectedKey(id(1));
    press({ key: "`", code: "Backquote", ctrl: true });
    expect(detailTab()).toBe("terminal");
    press({ key: "`", code: "Backquote", ctrl: true });
    expect(detailTab()).toBe("activity");
    expect(document.activeElement).toBe(editor);
  });

  it("Ctrl+` does nothing without a Terminal tab", () => {
    sessionsPage(["activity"]);
    setSelectedKey(id(1));
    press({ key: "`", code: "Backquote", ctrl: true });
    expect(detailTab()).toBe("activity");
  });

  it("F6 moves through the list, the view, the message field and the side panel", () => {
    const { list, view, editor, panel } = sessionsPage();
    setSelectedKey(id(1));
    const seen = [];
    for (let i = 0; i < 5; i++) { press({ key: "F6", code: "F6" }, document.activeElement ?? document.body); seen.push(document.activeElement); }
    expect(seen).toEqual([list, view, editor, panel, list]);
  });

  it("Shift+F6 moves backwards, from the message field to the view", () => {
    const { list, view, editor, panel } = sessionsPage();
    setSelectedKey(id(1));
    editor.focus();
    const seen = [];
    for (let i = 0; i < 4; i++) { press({ key: "F6", code: "F6", shift: true }, document.activeElement ?? document.body); seen.push(document.activeElement); }
    expect(seen).toEqual([view, list, panel, editor]);
  });

  it("gives every command that Go to and shortcut help list a handler", () => {
    for (const command of ["session.next", "session.previous", "session.nextAttention", "session.previousAttention", "session.rename", "session.search",
      "view.refresh", "tab.next", "tab.previous", "tab.terminal", "focus.next", "focus.previous"]) {
      expect(handlerFor(command, document.body), command).toBeTypeOf("function");
    }
  });
});
