import { compactRows, setCompactRows, type MenuEntry } from "../../store/ui";
import { grouping, setProjectCollapsed } from "../../store/organization";
import { clearArchiveMenuEntry } from "./selection-live";
import { markAllEntry } from "./mark-entries";

export interface BackgroundDeps {
  headers: () => { id: string }[];
  newProject: () => void;
  collapse: (project: string, collapsed: boolean) => void;
  expandRows: () => void;
  markAll: () => MenuEntry;
  clearArchive: () => MenuEntry;
}

const defaults: BackgroundDeps = {
  headers: () => grouping().headers,
  newProject: () => {},
  collapse: setProjectCollapsed,
  expandRows: () => Object.keys(compactRows).forEach((key) => setCompactRows(key, false)),
  markAll: markAllEntry,
  clearArchive: clearArchiveMenuEntry,
};

export function collapseAllProjects(headers: readonly { id: string }[], collapsed: boolean, set: BackgroundDeps["collapse"]) {
  for (const header of headers) set(header.id, collapsed);
}

export function expandAllCollapsedEntry(expandRows: () => void = defaults.expandRows): MenuEntry {
  return {
    label: "Expand all collapsed", icon: "expand-row", disabled: !Object.values(compactRows).some(Boolean),
    hint: "Restore full session rows, including sessions hidden by filters", action: expandRows,
  };
}

/** Qt's list background menu (SessionsWindow.cpp:340-373). */
export function backgroundEntries(overrides: Partial<BackgroundDeps> = {}): MenuEntry[] {
  const deps = { ...defaults, ...overrides };
  return [
    { label: "New session…", icon: "add", disabled: true },
    { label: "New project…", icon: "plus", action: deps.newProject },
    { separator: true },
    { label: "Expand all projects", icon: "expand-row", action: () => collapseAllProjects(deps.headers(), false, deps.collapse) },
    { label: "Collapse all projects", icon: "collapse-row", action: () => collapseAllProjects(deps.headers(), true, deps.collapse) },
    expandAllCollapsedEntry(deps.expandRows),
    { separator: true },
    deps.markAll(),
    deps.clearArchive(),
  ];
}
