import { describe, expect, it, vi } from "vitest";
import {
  chipColor, createOrgDialogs, deleteTargets, deleteText, editEntry, headerEntries, headerMenuState, moveChoices, nameProblem, projectColor,
  type HeaderActions, type OrgStore,
} from "./organization-ui";
import type { EditOutcome, GroupingOut } from "../../types";
import type { MenuEntry } from "../../store/ui";
import type { Session } from "./types";

const header = (id: string, extra = {}) => ({ id, name: id, color: null, accessible: true, collapsed: false, is_default: false, palette: 2, ...extra });
const g = (extra: Partial<GroupingOut> = {}): GroupingOut => ({
  loaded: true, default_project: "ungrouped", headers: [header("alpha"), header("ungrouped", { is_default: true }), header("beta", { collapsed: true })],
  ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0, ...extra,
});

describe("project menu rules", () => {
  it("moves up before the previous project and down before the one after the next", () => {
    const first = headerMenuState(g(), "alpha")!;
    expect([first.up.enabled, first.down.enabled, first.down.before]).toEqual([false, true, "beta"]);
    const middle = headerMenuState(g(), "ungrouped")!;
    expect([middle.up.before, middle.down.enabled, middle.down.before]).toEqual(["alpha", true, undefined]);
    const last = headerMenuState(g(), "beta")!;
    expect([last.down.enabled, last.collapse]).toEqual([false, "Expand project"]);
    expect(headerMenuState(g(), "gone")).toBeUndefined();
  });

  it("disables every write with the reason while the catalog is not ready, and Delete with one project", () => {
    const waiting = headerMenuState(g({ ready: false, reason: "Project sync is starting." }), "alpha")!;
    expect([waiting.edit, waiting.remove, waiting.down.enabled]).toEqual([
      { enabled: false, reason: "Project sync is starting." }, { enabled: false, reason: "Project sync is starting." }, false]);
    expect(waiting.collapse).toBe("Collapse project");
    const single = headerMenuState(g({ headers: [header("alpha")] }), "alpha")!;
    expect(single.remove.enabled).toBe(false);
  });

  it("offers every other project for the sessions of a deleted one, the default first", () => {
    expect(deleteTargets(g(), "alpha")).toMatchObject({ selected: "ungrouped" });
    expect(deleteTargets(g(), "ungrouped").options.map((h) => h.id)).toEqual(["alpha", "beta"]);
    expect(deleteTargets(g(), "ungrouped").selected).toBe("alpha");
  });

  it("checks the project every chosen row is in", () => {
    expect(moveChoices(g(), ["beta"]).filter((c) => c.current).map((c) => c.id)).toEqual(["beta"]);
    expect(moveChoices(g(), ["beta", "alpha"]).some((c) => c.current)).toBe(false);
  });

  it("colours by the project, else its palette slot, and wants a name", () => {
    expect(projectColor({ color: "#3fb950", palette: 1 })).toBe("#3fb950");
    expect(projectColor({ color: null, palette: 4 })).toBe("var(--z-entity-palette-4)");
    expect(nameProblem("  ")).toBe("Enter a project name.");
    expect(nameProblem(" Release ")).toBeUndefined();
  });

  it("colours a project chip as its header band, and a machine chip or an unlisted project by its name's slot", () => {
    const bands: Record<string, { color: string | null; palette: number }> = { alpha: { color: "#3fb950", palette: 1 }, beta: { color: null, palette: 4 } };
    const headerOf = (id: string) => bands[id];
    const slot = (name: string) => (name === "alpha" ? 6 : 7);
    expect(chipColor({ id: "alpha", label: "Alpha", kind: "p" }, headerOf, slot)).toBe("#3fb950");
    expect(chipColor({ id: "beta", label: "beta", kind: "p" }, headerOf, slot)).toBe("var(--z-entity-palette-4)");
    expect(chipColor({ id: "gone", label: "gone", kind: "p" }, headerOf, slot)).toBe("var(--z-entity-palette-7)");
    expect(chipColor({ id: "alpha", label: "alpha", kind: "m" }, headerOf, slot)).toBe("var(--z-entity-palette-6)");
  });
});

// The header menu's entries as Qt builds them (SessionsWindow.cpp:2568-2616).
type Item = Extract<MenuEntry, { label: string }>;
const labels = (entries: MenuEntry[]) => entries.map((e) => ("separator" in e ? "-" : "label" in e ? e.label : e.heading));
const entry = (entries: MenuEntry[], label: string) => entries.find((e): e is Item => "label" in e && e.label === label)!;
const actions = () => ({
  filter: vi.fn<() => void>(), collapse: vi.fn<(collapsed: boolean) => void>(), rename: vi.fn<() => void>(), settings: vi.fn<() => void>(),
  move: vi.fn<(before: string | undefined) => void>(), sort: vi.fn<() => void>(), remove: vi.fn<() => void>(),
}) satisfies HeaderActions;
const WAITING = "Project sync is starting.";

describe("the project header menu", () => {
  it("lists Qt's entries in Qt's order, with New session for phase 5", () => {
    const entries = headerEntries(g(), "alpha", actions());
    expect(labels(entries)).toEqual([
      "New session…", "-", "Filter this project", "-", "Collapse project", "Rename project…", "Project settings…", "-",
      "Move project up", "Move project down", "Sort sessions by name", "-", "Delete project…",
    ]);
    expect(entry(entries, "New session…").disabled).toBe(true);
    expect(entry(entries, "Delete project…").danger).toBe(true);
    expect(headerEntries(g(), "gone", actions())).toEqual([]);
  });

  it("filters to this project and toggles its collapse locally, also while sync waits (2575-2581)", () => {
    const act = actions();
    const waiting = headerEntries(g({ ready: false, reason: WAITING }), "alpha", act);
    for (const label of ["Filter this project", "Collapse project", "Project settings…"]) {
      expect(entry(waiting, label).disabled).toBeFalsy();
      entry(waiting, label).action!();
    }
    expect(act.filter).toHaveBeenCalledTimes(1);
    expect(act.collapse).toHaveBeenCalledWith(true);
    expect(act.settings).toHaveBeenCalledTimes(1);
    const collapsed = headerEntries(g(), "beta", act);
    expect(labels(collapsed)).toContain("Expand project");
    entry(collapsed, "Expand project").action!();
    expect(act.collapse).toHaveBeenLastCalledWith(false);
  });

  it("disables each write with the sync reason as its hint, and no hint once ready (Qt's hint)", () => {
    const writes = ["Rename project…", "Move project up", "Move project down", "Sort sessions by name", "Delete project…"];
    const waiting = headerEntries(g({ ready: false, reason: WAITING }), "ungrouped", actions());
    expect(writes.map((l) => [entry(waiting, l).disabled, entry(waiting, l).hint])).toEqual(writes.map(() => [true, WAITING]));
    const ready = headerEntries(g(), "ungrouped", actions());
    expect(writes.map((l) => [entry(ready, l).disabled, entry(ready, l).hint])).toEqual(writes.map(() => [false, undefined]));
  });

  it("moves up before the previous project, and down before the one after the next or to the end (2591-2600)", () => {
    const act = actions();
    const first = headerEntries(g(), "alpha", act);
    expect([entry(first, "Move project up").disabled, entry(first, "Move project down").disabled]).toEqual([true, false]);
    entry(first, "Move project down").action!();
    expect(act.move).toHaveBeenLastCalledWith("beta");
    const middle = headerEntries(g(), "ungrouped", act);
    entry(middle, "Move project up").action!();
    expect(act.move).toHaveBeenLastCalledWith("alpha");
    entry(middle, "Move project down").action!();
    expect(act.move).toHaveBeenLastCalledWith(undefined);
    expect(act.move).toHaveBeenCalledTimes(3);
    const last = headerEntries(g(), "beta", act);
    expect([entry(last, "Move project up").disabled, entry(last, "Move project down").disabled]).toEqual([false, true]);
    entry(last, "Move project up").action!();
    expect(act.move).toHaveBeenLastCalledWith("ungrouped");
  });

  it("renames, sorts and deletes this project, and Delete needs another project (2582-2614)", () => {
    const act = actions();
    const entries = headerEntries(g(), "alpha", act);
    entry(entries, "Rename project…").action!();
    entry(entries, "Sort sessions by name").action!();
    entry(entries, "Delete project…").action!();
    expect([act.rename, act.sort, act.remove].map((f) => f.mock.calls.length)).toEqual([1, 1, 1]);
    const single = headerEntries(g({ headers: [header("alpha")] }), "alpha", actions());
    expect([entry(single, "Delete project…").disabled, entry(single, "Delete project…").hint]).toEqual([true, undefined]);
    expect([entry(single, "Move project up").disabled, entry(single, "Move project down").disabled]).toEqual([true, true]);
    // The last project gets no reason, also while sync waits (2614).
    const lone = headerEntries(g({ headers: [header("alpha")], ready: false, reason: WAITING }), "alpha", actions());
    expect([entry(lone, "Delete project…").disabled, entry(lone, "Delete project…").hint]).toEqual([true, undefined]);
    expect(entry(lone, "Rename project…").hint).toBe(WAITING);
  });

  it("never separates values with dots or names a key in its texts", () => {
    const entries = [...headerEntries(g(), "alpha", actions()), ...headerEntries(g({ ready: false, reason: WAITING }), "beta", actions())];
    for (const e of entries) if ("label" in e) expect(`${e.label} ${e.hint ?? ""}`).not.toMatch(/[·•∙⋅]|Ctrl|Cmd|Shift|Alt\+/);
  });
});

describe("edit entries and names", () => {
  it("an edit waits for the catalog with its reason", () => {
    expect(editEntry(g())).toEqual({ enabled: true });
    expect(editEntry(g({ ready: false, reason: WAITING }))).toEqual({ enabled: false, reason: WAITING });
    expect(editEntry(g({ ready: false, reason: null }))).toEqual({ enabled: false });
  });

  it("wants a name that is not blank, as createGroup and renameGroup ignore one (135-136, 149)", () => {
    for (const blank of ["", " ", "\t\n "]) expect(nameProblem(blank)).toBe("Enter a project name.");
    expect(nameProblem("a")).toBeUndefined();
  });
});

// The dialogs' controller against a fake organization store.
const session = (rawMachine: string, name: string): Session => ({ key: `${rawMachine}\nname:${name}`, rawMachine, name, project: "sample-project" }) as Session;
const one = session("", "claude/sample-project/review");
const two = session("studio", "codex/aurora/orders-v3");

function fakeStore(grouping = g(), assigned: Record<string, string> = {}) {
  const done = (): Promise<EditOutcome> => Promise.resolve({ outcome: "done", conflicts: 0 });
  const store = {
    grouping: () => grouping,
    groupOf: (id: string) => assigned[id] ?? grouping.default_project,
    headerOf: (id: string) => grouping.headers.find((h) => h.id === id),
    moveToProject: vi.fn(done), createProject: vi.fn(done), renameProject: vi.fn(done), deleteProject: vi.fn(done),
  } satisfies OrgStore;
  const dialogs = createOrgDialogs(store);
  const calls = () => [store.moveToProject, store.createProject, store.renameProject, store.deleteProject].reduce((n, f) => n + f.mock.calls.length, 0);
  return { store, dialogs, calls };
}

describe("the Move to project dialog (SessionsWindow.cpp:5363-5372)", () => {
  it("is the session menu's entry, disabled with the reason while sync waits", () => {
    const { dialogs } = fakeStore();
    const e = dialogs.moveEntry(one) as Item;
    expect([e.label, e.icon, e.disabled, e.hint]).toEqual(["Move to project…", "folder", false, undefined]);
    e.action!();
    expect(dialogs.dialog()).toEqual({ kind: "move", rows: [one] });
    const waiting = fakeStore(g({ ready: false, reason: WAITING })).dialogs.moveEntry(one) as Item;
    expect([waiting.disabled, waiting.hint]).toEqual([true, WAITING]);
  });

  it("checks the project the organization puts the rows in, not their folder", () => {
    const { dialogs } = fakeStore(g(), { [one.key]: "beta", [two.key]: "beta" });
    expect(dialogs.moveChoicesFor([one]).map((c) => [c.id, c.current])).toEqual([["alpha", false], ["ungrouped", false], ["beta", true]]);
    expect(dialogs.moveChoicesFor([one, two]).find((c) => c.current)?.id).toBe("beta");
    const split = fakeStore(g(), { [one.key]: "alpha" });
    expect(split.dialogs.moveChoicesFor([one, two]).some((c) => c.current)).toBe(false);
    expect(split.dialogs.moveChoicesFor([two]).find((c) => c.current)?.id).toBe("ungrouped");
  });

  it("closes and moves every row with one call, also to the project they are in", () => {
    const { store, dialogs, calls } = fakeStore();
    dialogs.openMove([one, two]);
    dialogs.choose("beta");
    expect(dialogs.dialog()).toBeUndefined();
    expect(store.moveToProject).toHaveBeenCalledExactlyOnceWith([one, two], "beta");
    dialogs.openMove([one]);
    dialogs.choose("ungrouped");
    expect(store.moveToProject).toHaveBeenLastCalledWith([one], "ungrouped");
    expect(calls()).toBe(2);
  });

  it("New project from it creates the project with the rows in the same apply (2558-2566)", () => {
    const { store, dialogs, calls } = fakeStore();
    dialogs.openMove([one]);
    dialogs.newProject();
    expect(dialogs.dialog()).toEqual({ kind: "new", rows: [one] });
    expect(dialogs.submitName(" Release ")).toBe(true);
    expect(dialogs.dialog()).toBeUndefined();
    expect(store.createProject).toHaveBeenCalledExactlyOnceWith(" Release ", [one]);
    expect(calls()).toBe(1);
  });

  it("does nothing on a choice once it closed", () => {
    const { dialogs, calls } = fakeStore();
    dialogs.choose("beta");
    dialogs.openRename("alpha");
    dialogs.choose("beta");
    expect(calls()).toBe(0);
  });
});

describe("the name dialogs (2558-2566, 2582-2587)", () => {
  it("New project creates an empty project, and a blank name keeps the dialog open without a call", () => {
    const { store, dialogs, calls } = fakeStore();
    dialogs.openNew();
    expect(dialogs.dialog()).toEqual({ kind: "new", rows: [] });
    expect(dialogs.nameDialog()).toEqual({ title: "New project", initial: "" });
    expect(dialogs.submitName("   ")).toBe(false);
    expect(dialogs.dialog()?.kind).toBe("new");
    expect(calls()).toBe(0);
    dialogs.submitName("Release");
    expect(store.createProject).toHaveBeenCalledExactlyOnceWith("Release", []);
  });

  it("Rename starts from the current name and renames this project", () => {
    const { store, dialogs, calls } = fakeStore(g({ headers: [header("alpha", { name: "Alpha" }), header("beta")] }));
    dialogs.openRename("alpha");
    expect(dialogs.nameDialog()).toEqual({ title: "Rename project", initial: "Alpha" });
    expect(dialogs.submitName("")).toBe(false);
    expect(calls()).toBe(0);
    expect(dialogs.submitName("Alpha two")).toBe(true);
    expect(dialogs.dialog()).toBeUndefined();
    expect(store.renameProject).toHaveBeenCalledExactlyOnceWith("alpha", "Alpha two");
    expect(calls()).toBe(1);
  });

  it("Rename does not open for a project that is no longer listed (2584)", () => {
    const { dialogs } = fakeStore();
    dialogs.openRename("gone");
    expect(dialogs.dialog()).toBeUndefined();
    expect(dialogs.nameDialog()).toBeUndefined();
  });
});

describe("the Delete project dialog (ProjectsDialog.cpp:367-381)", () => {
  it("names the project, offers every other one with the default chosen, and moves its sessions there", () => {
    const { store, dialogs, calls } = fakeStore(g({ headers: [header("alpha", { name: "Alpha" }), header("ungrouped", { name: "General" }), header("beta")] }));
    dialogs.openDelete("alpha");
    const open = dialogs.dialog();
    expect(open).toMatchObject({ kind: "delete", project: "alpha", name: "Alpha", selected: "ungrouped" });
    expect(open?.kind === "delete" && open.options.map((h) => h.id)).toEqual(["ungrouped", "beta"]);
    expect(deleteText("Alpha")).toBe("Delete “Alpha”? Choose the project that will receive its sessions. Files and running agents stay in place.");
    dialogs.confirmDelete("beta");
    expect(dialogs.dialog()).toBeUndefined();
    expect(store.deleteProject).toHaveBeenCalledExactlyOnceWith("alpha", "beta");
    expect(calls()).toBe(1);
  });

  it("chooses the default project wherever it is listed (374)", () => {
    const { dialogs } = fakeStore();
    dialogs.openDelete("beta");
    expect(dialogs.dialog()).toMatchObject({ selected: "ungrouped" });
    expect(deleteTargets(g({ default_project: "beta" }), "ungrouped").selected).toBe("beta");
  });

  it("chooses the first other project when the default is the one deleted, and needs a target", () => {
    const { store, dialogs } = fakeStore();
    dialogs.openDelete("ungrouped");
    expect(dialogs.dialog()).toMatchObject({ selected: "alpha" });
    dialogs.confirmDelete("");
    expect(dialogs.dialog()?.kind).toBe("delete");
    expect(store.deleteProject).not.toHaveBeenCalled();
    dialogs.close();
    dialogs.confirmDelete("beta");
    expect(store.deleteProject).not.toHaveBeenCalled();
  });

  it("does not open with one project or for one no longer listed (369)", () => {
    const single = fakeStore(g({ headers: [header("alpha")] }));
    single.dialogs.openDelete("alpha");
    expect(single.dialogs.dialog()).toBeUndefined();
    const gone = fakeStore();
    gone.dialogs.openDelete("gone");
    expect(gone.dialogs.dialog()).toBeUndefined();
  });
});
