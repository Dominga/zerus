import type { ComputerId } from "../types";
import type { Session } from "../ui/sessions/types";

// Task 252: the session rows' git status (Qt HgsClient gitStatusSnapshot,
// SessionPresentation::gitStatus and GitStatusBadge). The desktop reads
// `hgs git-status` for the folders the visible rows show (src-tauri
// git_status.rs, core git_status.rs); this module keeps the answers and maps
// one to a row's `git` with Qt's rules. It imports no store, so the catalog's
// toSession can read it.

export type GitSnapshot = Record<string, unknown>;
export type RowGit = NonNullable<Session["git"]>;

const answers = new Map<string, GitSnapshot>();
const asked = new Set<string>();

export const gitKey = (computer: ComputerId, path: string) => `${computer}\n${path}`;

/** Qt gitStatusPath: the checkout's root, else its folder; none for an archived row. */
export function gitPath(s: Pick<Session, "state" | "gitRoot" | "cwdCanonical" | "cwdPath">): string {
  if (s.state === "archived") return "";
  return s.gitRoot || s.cwdCanonical || s.cwdPath || "";
}

const num = (value: unknown) => (typeof value === "number" && Number.isFinite(value) ? value : undefined);

/** Qt GitStatusBadge::fresh: an `ok` answer under 30 s old with its changed files, on a computer online. */
export function fresh(data: GitSnapshot, now: number, online: boolean): boolean {
  const age = now - (num(data.received_at_ms) ?? -Infinity);
  return online && age >= 0 && age < 30_000 && data.state === "ok" && (num(data.changed_files) ?? -1) >= 0;
}

/** Qt GitStatusBadge::verified: fresh, and the remote checked under 60 s ago. */
export function verified(data: GitSnapshot, now: number, online: boolean): boolean {
  const remote = num(data.remote_age_seconds);
  const age = now - (num(data.received_at_ms) ?? 0);
  return fresh(data, now, online) && data.remote_state === "verified" && remote !== undefined && remote >= 0 && remote + age / 1000 < 60;
}

/**
 * A row's git from its folder's answer (SessionPresentation::gitStatus, then
 * GitStatusBadge::marks): none outside Git; an answer for another branch
 * reads as not checked; counts from the answer; `unknown` unless the remote
 * is verified, else what the checkout shows first (changes, outgoing,
 * incoming) or `synced`.
 */
export function gitOf(data: GitSnapshot | undefined, s: Pick<Session, "branch" | "gitRoot">, now: number, online: boolean): RowGit | undefined {
  if (!data) return { ahead: 0, behind: 0, changed: 0, state: "unknown" };
  if (data.state === "not_repo") return undefined;
  const sameScope = data.state !== "ok" || ((!s.gitRoot || !data.root || data.root === s.gitRoot) && (!data.branch || !s.branch || data.branch === s.branch));
  const changed = Math.max(0, num(data.changed_files) ?? 0), ahead = Math.max(0, num(data.ahead) ?? 0), behind = Math.max(0, num(data.behind) ?? 0);
  const conflicts = Math.max(0, num(data.conflicts) ?? 0);
  const remote = sameScope && verified(data, now, online);
  const state: RowGit["state"] = !remote ? "unknown" : changed ? "diff" : ahead ? "push" : behind ? "pull" : "synced";
  return { ahead, behind, changed, ...(conflicts ? { conflicts } : {}), state };
}

/** A row's git now: none until its folder was asked for; `unknown` while the first answer is on its way. */
export function gitFor(s: Pick<Session, "computer" | "state" | "branch" | "gitRoot" | "cwdCanonical" | "cwdPath">, online: boolean, now = Date.now()): RowGit | undefined {
  const path = gitPath(s);
  const key = gitKey(s.computer, path);
  if (!path || !asked.has(key)) return undefined;
  return gitOf(answers.get(key), s, now, online);
}

/** The folders the desktop was asked for. */
export function markAsked(computer: ComputerId, paths: readonly string[]) {
  for (const path of paths) if (path) asked.add(gitKey(computer, path));
}

/** An answer from the desktop (its event or its request's reply). */
export function storeAnswer(computer: ComputerId, path: string, status: GitSnapshot) {
  answers.set(gitKey(computer, path), status);
  asked.add(gitKey(computer, path));
}

/** Tests: forget everything. */
export function resetGitStatus() {
  answers.clear();
  asked.clear();
}

/** The folders of visible rows a computer should read now, each once (Qt SessionsWindowRows gitReads): online, live rows with a folder. */
export function visibleFolders(rows: readonly Pick<Session, "computer" | "state" | "gitRoot" | "cwdCanonical" | "cwdPath">[], online: (computer: ComputerId) => boolean): Map<ComputerId, string[]> {
  const out = new Map<ComputerId, string[]>();
  for (const row of rows) {
    const path = gitPath(row);
    if (!path || !online(row.computer)) continue;
    const list = out.get(row.computer) ?? [];
    if (!list.includes(path)) list.push(path);
    out.set(row.computer, list);
  }
  return out;
}
