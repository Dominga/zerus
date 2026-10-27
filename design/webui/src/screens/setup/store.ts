import { createSignal } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { projects as seedProjects } from "../../mock/data";
import { seedCreations, overviewRows, type CreationRecord, type OverviewRow } from "../../mock/setup-extra";
import type { Project, ProjectFolder } from "../../mock/types";
import { toast } from "../../state";

// Mockup state shared by the Projects page and the setup dialogs. In the
// Tauri client this is the organization catalog published by the backend;
// every edit is one request and the page re-renders from the next snapshot.

export interface EditableProject extends Project {
  /** A custom color chosen in the color popup, else the entity palette slot. */
  color?: string;
}

const clone = (p: Project): EditableProject => ({ ...p, folders: p.folders.map((f) => ({ ...f })) });

export const [org, setOrg] = createStore<{ projects: EditableProject[]; saved: string }>({
  projects: seedProjects.map(clone),
  saved: "",
});

export const projectById = (id: string | undefined) => org.projects.find((p) => p.id === id);
export const defaultProject = () => org.projects.find((p) => p.isDefault) ?? org.projects[0];

/** CSS value for a project's color: a chosen hex, else its palette slot. */
export const projectColor = (p: Pick<EditableProject, "color" | "colorIndex"> | undefined) =>
  p?.color ?? `var(--z-entity-palette-${p?.colorIndex ?? 7})`;

export const [selectedProject, setSelectedProject] = createSignal<string>("aurora");
export const [selectedFolder, setSelectedFolder] = createSignal<number>(0);

let savedTimer: ReturnType<typeof setTimeout> | undefined;
/** The status line reads "Saved" after each change (ProjectsDialog.cpp:313). */
export function markSaved(text = "Saved") {
  setOrg("saved", text);
  clearTimeout(savedTimer);
  savedTimer = setTimeout(() => setOrg("saved", ""), 6000);
}

export function updateProject(id: string, patch: Partial<EditableProject>) {
  setOrg("projects", (p) => p.id === id, patch);
  markSaved();
}

export function addProject(name: string): string {
  const id = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || `project-${org.projects.length + 1}`;
  const unique = projectById(id) ? `${id}-${org.projects.length + 1}` : id;
  setOrg("projects", (list) => [...list, { id: unique, name, colorIndex: org.projects.length % 7, vivid: false, isDefault: false, folders: [] }]);
  markSaved();
  return unique;
}

export function moveProject(id: string, delta: number) {
  const i = org.projects.findIndex((p) => p.id === id);
  const j = i + delta;
  if (i < 0 || j < 0 || j >= org.projects.length) return;
  setOrg("projects", produce((list) => { const [p] = list.splice(i, 1); list.splice(j, 0, p); }));
  markSaved();
}

export function moveProjectTo(id: string, before: number) {
  const i = org.projects.findIndex((p) => p.id === id);
  if (i < 0) return;
  setOrg("projects", produce((list) => { const [p] = list.splice(i, 1); list.splice(before > i ? before - 1 : before, 0, p); }));
  markSaved();
}

export function setDefaultProject(id: string) {
  setOrg("projects", produce((list) => list.forEach((p) => { p.isDefault = p.id === id; })));
  markSaved();
}

export function deleteProject(id: string, target: string) {
  const name = projectById(id)?.name;
  const wasDefault = projectById(id)?.isDefault;
  setOrg("projects", (list) => list.filter((p) => p.id !== id));
  if (wasDefault) setDefaultProject(target);
  setSelectedProject(target);
  setSelectedFolder(0);
  markSaved();
  toast("Project deleted", `Sessions of “${name}” moved to “${projectById(target)?.name}”. Files and running agents stay in place.`);
}

export function addFolder(projectId: string, folder: ProjectFolder) {
  setOrg("projects", (p) => p.id === projectId, "folders", (list) => [...list, folder]);
  markSaved();
}

export function replaceFolder(projectId: string, index: number, folder: ProjectFolder) {
  setOrg("projects", (p) => p.id === projectId, "folders", index, folder);
  markSaved();
}

export function removeFolder(projectId: string, index: number) {
  setOrg("projects", (p) => p.id === projectId, "folders", (list) => list.filter((_, i) => i !== index));
  markSaved();
}

export const hasFolder = (projectId: string, machine: string, path: string, except = -1) =>
  !!projectById(projectId)?.folders.some((f, i) => i !== except && f.machine === machine && f.path === path);

// ---------------------------------------------------------------------------
// New session prefill: what opened the dialog (Projects, a worktree row...).

export interface LaunchPreset { project?: string; machine?: string; path?: string; agent?: string; repository?: boolean; restore?: CreationRecord }
export const [launchPreset, setLaunchPreset] = createSignal<LaunchPreset>({});

// Folder dialog context: add (index -1) or edit an existing link.
export const [folderEdit, setFolderEdit] = createSignal<{ project: string; index: number }>({ project: "aurora", index: -1 });

// Clone flow: chosen computer, and who asked (Projects or a directory pick).
export const [cloneMachine, setCloneMachine] = createSignal("workstation");

// Directory picker: which computer, the start path and where the result goes.
export const [dirPick, setDirPick] = createSignal<{ machine: string; path: string; onPick?: (path: string) => void }>({ machine: "macbook", path: "/Users/example/src" });

// Worktree review context.
export const [overview, setOverview] = createStore<{ rows: OverviewRow[]; selected: string[]; focus: string }>({
  rows: overviewRows.map((r) => ({ ...r })),
  selected: [],
  focus: "wt5",
});
export const [batchTargets, setBatchTargets] = createSignal<string[]>(["wt1", "wt2", "wt3", "wt5", "wt8", "wt4"]);
export const [batchExit, setBatchExit] = createSignal(false);
export const [cleanupFocus, setCleanupFocus] = createSignal<string | undefined>();
export const [inspectTarget, setInspectTarget] = createSignal("wt5");

// Session creation journal.
export const [creations, setCreations] = createStore<{ items: CreationRecord[]; selected: string }>({
  items: seedCreations.map((c, i) => {
    const createdAt = Date.now() - (i + 1) * 1800e3;
    return { ...c, history: [...c.history], createdAt, startedAt: createdAt, endedAt: createdAt + [63e3, 4e3, 70e3][i] };
  }),
  selected: "c-2",
});

// Swarm dialog.
export const [swarmTab, setSwarmTab] = createSignal<"computers" | "connect" | "conflicts">("computers");
export const [swarmPeer, setSwarmPeer] = createSignal("win-lab");
export const [resolvedConflicts, setResolvedConflicts] = createStore<Record<string, boolean>>({});
