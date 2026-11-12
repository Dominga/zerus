import { createSignal, onCleanup } from "solid-js";
import * as bridge from "../../bridge";
import * as organization from "../../store/organization";
import { machineCards } from "../../store/machines";
import { bus } from "../../store/notices";
import type { Folder, FolderOut, PathStyle, Project } from "../../types";
import { selectedStyle } from "../sessions/path-forms";

import { trNoop, tr } from "../../i18n";
import { teamConfirm } from "../../store/teamConfirm";

export interface Place { computer: string; host: string; machine_id: string | null; label: string; style: PathStyle }
export type FolderChange =
  | { kind: "add"; place: Place; path: string; name: string }
  | { kind: "edit"; original: FolderOut; place: Place; path: string; name: string }
  | { kind: "remove"; original: FolderOut }
  | { kind: "transfer"; original: FolderOut; destination: string; copy: boolean };
export interface FolderPrompt { project: string; original?: FolderOut; places: Place[]; origin: HTMLElement }
const CHANGED = trNoop("This project folder changed. Select it again.");
const refuse = (text: string) => bus.notify("warn", text);

/** Qt's ProjectsDialog::removeFolder question: the folder's name (else its last path part), its path, machine and project. */
export function folderRemovalCopy(folder: Folder, project: string): { title: string; text: string; action: string } {
  const name = folder.name || folder.path.split(/[\/\\]/u).filter(Boolean).at(-1) || folder.path;
  return {
    title: tr("Remove folder?"),
    text: tr("Remove “%1” (%2 on %3) from %4? Files stay on disk.", name, folder.path, folder.machine || folder.computer || tr("this computer"), project),
    action: tr("Remove folder"),
  };
}
export function createProjectFolders(projects: () => Project[], select: (key: string) => void) {
  const [prompt, setPrompt] = createSignal<FolderPrompt>();
  const [busy, setBusy] = createSignal(false);
  let left = false; onCleanup(() => { left = true; });
  // N6: a folder of this computer is edited like a peer's (the local Windows computer included).
const enabled = (project: string, _folder?: Folder) => !busy() && organization.grouping().ready && !!projects().find((p) => p.id === project && p.accessible);
  const places = (): Place[] => Object.entries(machineCards).filter(([computer, m]) => !!m.host && (computer !== "" || m.local_sessions !== false)).map(([computer, m]) => ({ computer, host: m.host!, machine_id: m.machine_id ?? null, label: computer || `${m.host} (this computer)`, style: selectedStyle(m.platform, computer === "", "posix") }));
  const read = async (project: string, folder: Folder) => {
    const current = projects().find((p) => p.id === project)?.folders.find((f) => f.key === folder.key);
    if (!current || current.machine !== folder.machine || current.path !== folder.path || current.name !== folder.name || current.machine_id !== folder.machine_id) throw Error(tr(CHANGED));
    const raw = (await bridge.projectFolders(project)).filter((f) => f.key === folder.key);
    if (raw.length !== 1 || raw[0].machine !== folder.machine || raw[0].path !== folder.path || raw[0].machine_id !== (folder.machine_id ?? "") || (raw[0].name || raw[0].path.split(/[\/\\]/u).filter(Boolean).at(-1) || raw[0].path) !== folder.name || (raw[0].machine_name || raw[0].machine) !== folder.computer) throw Error(tr(CHANGED));
    return raw[0];
  };
  const close = () => { if (!busy()) setPrompt(undefined); };
  async function open(project: string, folder: Folder | undefined, origin: HTMLElement) {
    if (!enabled(project, folder)) return;
    setBusy(true);
    try {
      const original = folder ? await read(project, folder) : undefined;
      if (!left) setPrompt({ project, original, places: places(), origin });
    } catch (error) { refuse(String(error)); } finally { setBusy(false); }
  }
  async function save(place: Place, path: string, name: string) {
    const p = prompt(); if (!p || !enabled(p.project)) return;
    setBusy(true);
    try {
      const out = await organization.changeProjectFolder(p.project, p.original ? { kind: "edit", original: p.original, place, path, name } : { kind: "add", place, path, name });
      if (!left && out.outcome !== "refused") { if (out.folder) select(out.folder); setPrompt(undefined); }
    } finally { if (!left) setBusy(false); }
  }
  async function change(project: string, folder: Folder, destination?: string, copy = false) {
    if (!enabled(project, folder) || (destination && !enabled(destination))) return;
    setBusy(true);
    try {
      const original = await read(project, folder);
      if (left) return;
      await organization.changeProjectFolder(project, destination ? { kind: "transfer", original, destination, copy } : { kind: "remove", original });
    } catch (error) { refuse(String(error)); } finally { if (!left) setBusy(false); }
  }
  async function openFolder(project: string, folder: Folder) {
    if (!enabled(project, folder)) return;
    try { await bridge.projectFolderOpen(project, await read(project, folder)); }
    catch (error) { refuse(String(error)); }
  }
  async function remove(project: string, folder: Folder) {
    if (!enabled(project, folder)) return;
    const found = projects().find((candidate) => candidate.id === project);
    if (!found || !(await teamConfirm.ask({ name: "removeProjectFolderConfirm", ...folderRemovalCopy(folder, found.name) }))) return;
    return change(project, folder);
  }
  return { prompt, busy, enabled, open, close, save, remove, transfer: (project: string, folder: Folder, target: string, copy: boolean) => change(project, folder, target, copy), openFolder };
}
