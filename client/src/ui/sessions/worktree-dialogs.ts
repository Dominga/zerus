import type { CreatedWorktree } from "../../types";
import { array, object, string, type Json } from "../lib/json";
import { absolute, join, same, split, type Style } from "./paths";

// New session's two worktree dialogs, as Qt keeps them in their views:
// Choose existing… is WorktreePanel in picker mode (tray/src/WorktreePanel.h)
// and Create manually… is NewWorktreeDialog (tray/src/NewWorktreeDialog.h).
// WorktreePicker.tsx and NewWorktreeDialog.tsx render them. The picker only
// reads (`worktrees --path P --json [--refresh]`); Create worktree is the one
// call that creates, sent once per click with no expected machine, as Qt
// sends it. Answers for another request change nothing.

const errorText = (error: unknown) => (typeof error === "string" ? error : error instanceof Error ? error.message : String(error));
const flag = (value: Json, key: string) => value[key] === true;

// Choose existing… (WorktreePanel in picker mode).

/** A listed session of the computer, as the picker counts it (G1). */
export interface PickerSession { gitRoot: string; cwdCanonical: string }

/** One checkout or folder the picker lists. */
export interface WorktreeRow {
  path: string;
  /** "Main checkout / main (1 session)" and the like. */
  label: string;
  /** The path, then the locked and prunable reasons, one per line. */
  tooltip: string;
  /** Available, not bare and current: Use folder takes it. */
  usable: boolean;
}

export interface WorktreePickerView {
  status: string;
  rows: WorktreeRow[];
  /** The selected row's path; undefined without rows. */
  selected: string | undefined;
  refreshEnabled: boolean;
  useEnabled: boolean;
}

/** The catalog lists at most this many worktrees (Partial catalog). */
export const MAX_WORKTREES = 512;
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const two = (n: number) => String(n).padStart(2, "0");

/** Qt's "d MMM HH:mm:ss" in local time, with C-locale month names. */
export function snapshotTime(seconds: number): string {
  const at = new Date(seconds * 1000);
  return `${at.getDate()} ${MONTHS[at.getMonth()]} ${two(at.getHours())}:${two(at.getMinutes())}:${two(at.getSeconds())}`;
}

/** A row's label (WorktreePanel::render): its kind, branch, marks and its session count. */
export function worktreeLabel(row: Json, sessions: number): string {
  const kind = string(row.kind);
  let label = kind === "main" ? "Main checkout" : kind === "linked" ? "Linked worktree" : kind === "bare" ? "Bare repository" : "Folder";
  const branch = string(row.branch);
  if (branch) label += ` / ${branch}`;
  else if (flag(row, "detached")) label += ` / detached ${string(row.head).slice(0, 8)}`;
  if (flag(row, "locked")) label += " (locked)";
  if (flag(row, "prunable")) label += " (prunable)";
  if (!flag(row, "available")) label += " (unavailable)";
  return label + (sessions === 1 ? " (1 session)" : ` (${sessions} sessions)`);
}

/**
 * The picker of one computer's folder, fixed for its life (`setContext` at
 * open). It shows the folder's snapshot, then reads its catalog once it is
 * shown on an online computer; Refresh reads it again. `online`, the
 * computer's sessions and its style are the caller's at each view.
 */
export class WorktreePicker {
  onChange?: () => void;

  private data: Json = {};
  private error = "";
  private request = 0;
  private ticket = 0;
  private answered = false;
  private selection: string | undefined;
  private closed = false;

  constructor(
    private readonly calls: { snapshot(): Promise<Json>; catalog(refresh: boolean): Promise<Json> },
    private readonly path: string,
    /** The computer as the status names it: the alias, else this computer's host name. */
    private readonly machine: string,
  ) {
    calls.snapshot().then((saved) => {
      if (this.closed || this.answered) return;
      this.data = object(saved);
      this.changed();
    }, () => {});
  }

  /** Shown (`showEvent`) or Refresh: reads the catalog unless no folder, an offline computer or a read on its way. */
  private read(online: boolean, refresh: boolean) {
    if (this.path === "" || !online || this.request) return;
    this.error = "";
    const request = ++this.ticket;
    this.request = request;
    this.calls.catalog(refresh).then((data) => {
      if (this.closed || request !== this.request) return;
      this.request = 0;
      this.answered = true;
      this.data = object(data);
      this.error = "";
      this.changed();
    }, (failure) => {
      if (this.closed || request !== this.request) return;
      this.request = 0;
      this.error = errorText(failure);
      this.changed();
    });
    this.changed();
  }

  show(online: boolean) { this.read(online, false); }
  refresh(online: boolean) { this.read(online, true); }

  select(path: string) {
    this.selection = path;
    this.changed();
  }

  dispose() { this.closed = true; }

  view(online: boolean, sessions: readonly PickerSession[], style: Style): WorktreePickerView {
    const data = this.data;
    const state = string(data.state);
    const stale = !online || flag(data, "stale") || this.error !== "";
    let status: string;
    if (this.path === "") status = "Select a folder.";
    else if (!online) status = "Offline. Showing the last snapshot.";
    else if (this.error) status = `Catalog unavailable: ${this.error}`;
    else if (this.request) status = "Reading folder…";
    else if (state === "not_repo") status = "Ordinary folder (no Git).";
    else if (state !== "ok") status = state === "folder_unavailable" ? "Folder is missing or inaccessible." : "Git information unavailable. You can still use an accessible folder.";
    else if (flag(data, "partial")) status = "Partial catalog (first 512 worktrees).";
    else status = `Existing working folders on ${this.machine}`;
    const sampled = typeof data.sampled_at === "number" ? data.sampled_at : 0;
    if (sampled > 0) status += `\nSnapshot: ${snapshotTime(sampled)}${stale ? " (outdated)" : ""}`;

    let roots = array(data.worktrees).slice(0, MAX_WORKTREES).map(object);
    if (!roots.length && this.path !== "") {
      roots = [{ path: typeof data.path === "string" ? data.path : this.path, kind: "folder", available: state !== "folder_unavailable" && Object.keys(data).length > 0 }];
    }
    const rows = roots.map((row): WorktreeRow => {
      const path = string(row.path);
      const checkout = row.kind !== "folder";
      const count = sessions.filter((s) => same(checkout ? s.gitRoot : s.cwdCanonical, path, style)).length;
      return {
        path, label: worktreeLabel(row, count),
        tooltip: `${path}\n${string(row.locked_reason)}\n${string(row.prunable_reason)}`,
        usable: flag(row, "available") && row.kind !== "bare" && !stale,
      };
    });
    // The kept choice, else the opened folder or its root (the last such row), else the first.
    let selected = rows.find((r) => r.path === this.selection);
    if (!selected && this.selection === undefined) {
      const root = string(data.selected_root);
      selected = rows.filter((r) => r.path === this.path || r.path === root).at(-1);
    }
    selected ??= rows[0];
    const current = online && !this.request && this.error === "" && !flag(data, "stale");
    return {
      status, rows, selected: selected?.path,
      refreshEnabled: online && !this.request && this.path !== "",
      useEnabled: !!selected?.usable && current,
    };
  }

  private changed() { if (!this.closed) this.onChange?.(); }
}

// Create manually… (NewWorktreeDialog).

/** The branch as a folder name: runs of anything but letters, digits, '.', '_' and '-' become '-'. */
export const branchFolder = (branch: string) => branch.trim().replace(/[^\p{L}\p{N}._-]+/gu, "-");

export interface NewWorktreeView {
  context: string;
  branch: string;
  base: string;
  /** Start from's list: HEAD, then the catalog's branches. */
  bases: string[];
  destination: string;
  /** "", "Creating worktree…" or the failure. */
  error: string;
  busy: boolean;
  createEnabled: boolean;
}

/**
 * New worktree for one computer and folder, fixed for its life. The folder
 * suggestion is `{parent of the selected root}/{repository}-{branch}` in the
 * computer's path style (deviation 3) until the folder is edited.
 */
export class NewWorktree {
  onChange?: () => void;
  /** Create worktree succeeded: the new checkout and its verified catalog. */
  onCreated?: (created: CreatedWorktree) => void;

  private branch: string;
  private base = "HEAD";
  private destination = "";
  private custom = false;
  private parent: string;
  private repository: string;
  private error = "";
  private request = 0;
  private ticket = 0;
  private readonly bases: string[];

  constructor(
    private readonly create_: (branch: string, base: string, destination: string) => Promise<CreatedWorktree>,
    private readonly machine: string,
    private readonly source: string,
    catalog: Json,
    branch: string,
    private readonly style: Style,
  ) {
    this.branch = branch;
    this.bases = ["HEAD"];
    for (const entry of array(catalog.worktrees)) {
      const name = string(object(entry).branch);
      if (name && !this.bases.includes(name)) this.bases.push(name);
    }
    const root = string(catalog.selected_root) || source;
    [this.parent, this.repository] = split(root, style);
    this.suggest();
  }

  view(): NewWorktreeView {
    const busy = this.request !== 0;
    return {
      context: `Computer: ${this.machine}\nRepository: ${this.source}`,
      branch: this.branch, base: this.base, bases: this.bases, destination: this.destination, error: this.error, busy,
      createEnabled: !busy && this.branch.trim() !== "" && this.base.trim() !== "" && absolute(this.destination, this.style),
    };
  }

  /** Closing is refused while the worktree is being created. */
  canClose() { return this.request === 0; }

  setBranch(text: string) {
    if (this.request) return;
    this.branch = text;
    this.suggest();
    this.changed();
  }

  setBase(text: string) {
    if (this.request) return;
    this.base = text;
    this.changed();
  }

  /** The folder edited by hand: the suggestion stops following the branch. */
  setDestination(text: string) {
    if (this.request) return;
    this.custom = true;
    this.destination = text;
    this.changed();
  }

  /** Where Parent folder… starts: the folder's parent. */
  browseStart() { return split(this.destination, this.style)[0]; }

  /** Parent folder… chose `folder`: the suggestion follows the branch again under it. */
  browsed(folder: string) {
    if (!folder || this.request) return;
    this.parent = folder;
    this.custom = false;
    this.suggest();
    this.changed();
  }

  /** Create worktree: one request; its failure is shown, never sent again. */
  create() {
    if (!this.view().createEnabled) return;
    this.error = "Creating worktree…";
    const request = ++this.ticket;
    this.request = request;
    this.create_(this.branch.trim(), this.base.trim(), this.destination).then((created) => {
      if (request !== this.request) return;
      this.request = 0;
      this.changed();
      this.onCreated?.(created);
    }, (failure) => {
      if (request !== this.request) return;
      this.request = 0;
      this.error = errorText(failure);
      this.changed();
    });
    this.changed();
  }

  private suggest() {
    if (this.custom) return;
    this.destination = join(this.parent, `${this.repository}-${branchFolder(this.branch)}`);
  }

  private changed() { this.onChange?.(); }
}
