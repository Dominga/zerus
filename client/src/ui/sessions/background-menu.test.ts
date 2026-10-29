import { describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  const store = new Map<string, string>();
  vi.stubGlobal("location", new URL("http://localhost/"));
  vi.stubGlobal("localStorage", { getItem: (key: string) => store.get(key) ?? null, setItem: (key: string, value: string) => { store.set(key, value); }, removeItem: (key: string) => { store.delete(key); } });
});

import { backgroundEntries, collapseAllProjects, expandAllCollapsedEntry, type BackgroundDeps } from "./background-menu";

function harness() {
  const calls: string[] = [];
  const deps: BackgroundDeps = {
    headers: () => [{ id: "p-one" }, { id: "p-two" }],
    newSession: () => calls.push("new-session"),
    newProject: () => calls.push("new-project"),
    collapse: (project, collapsed) => calls.push(`${project}:${collapsed}`),
    expandRows: () => calls.push("expand-rows"),
    markAll: () => ({ label: "Mark all as read", action: () => calls.push("mark-all") }),
    clearArchive: () => ({ label: "Clear archive…", action: () => calls.push("clear-archive") }),
  };
  return { deps, calls };
}

describe("the Sessions list background menu (SessionsWindow.cpp:340-373)", () => {
  it("opens New session and New project", () => {
    const { deps, calls } = harness();
    const entries = backgroundEntries(deps);
    expect(entries.map((entry) => "label" in entry ? entry.label : "separator")).toEqual([
      "New session…", "New project…", "separator", "Expand all projects", "Collapse all projects", "Expand all collapsed", "separator", "Mark all as read", "Clear archive…",
    ]);
    expect("disabled" in entries[0] && entries[0].disabled).toBe(false);
    expect("action" in entries[0] && entries[0].action).toBeTypeOf("function");
    expect("action" in entries[1] && entries[1].action).toBeTypeOf("function");
    if ("action" in entries[0]) entries[0].action?.();
    if ("action" in entries[1]) entries[1].action?.();
    expect(calls).toEqual(["new-session", "new-project"]);
  });

  it("expands and collapses every project through the layout API", () => {
    const { deps, calls } = harness();
    const entries = backgroundEntries(deps);
    if ("action" in entries[3]) entries[3].action?.();
    if ("action" in entries[4]) entries[4].action?.();
    expect(calls).toEqual(["p-one:false", "p-two:false", "p-one:true", "p-two:true"]);
    const other: string[] = [];
    collapseAllProjects([{ id: "p-one" }, { id: "p-two" }], true, (project) => other.push(project));
    expect(other).toEqual(["p-one", "p-two"]);
  });

  it("wires Expand all collapsed, Mark all as read and Clear archive", () => {
    const { deps, calls } = harness();
    const entries = backgroundEntries(deps);
    expect(entries[5]).toEqual(expandAllCollapsedEntry(deps.expandRows));
    expect(entries[5]).toMatchObject({
      disabled: true,
      hint: "Restore full session rows, including sessions hidden by filters",
    });
    if ("action" in entries[5]) entries[5].action?.();
    if ("action" in entries[7]) entries[7].action?.();
    if ("action" in entries[8]) entries[8].action?.();
    expect(calls).toEqual(["expand-rows", "mark-all", "clear-archive"]);
  });

  it("collapses projects hidden by the empty-project and scope filters", () => {
    const { deps, calls } = harness();
    deps.headers = () => [{ id: "p-visible" }, { id: "p-empty-hidden" }, { id: "p-outside-filter" }];
    const entries = backgroundEntries(deps);
    if ("action" in entries[4]) entries[4].action?.();
    expect(calls).toEqual(["p-visible:true", "p-empty-hidden:true", "p-outside-filter:true"]);
  });
});
