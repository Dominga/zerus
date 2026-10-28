import type { LaunchProject, PathStyle } from "../../types";
import { array, object, string, type Json } from "../lib/json";

// Folder paths in the form of the computer they belong to, and New session's
// folder helpers: a line-for-line port of client/core/src/paths.rs (`of`,
// `absolute`, `clean`, `same`, `within`, `leaf`, `native`, `split`, `join`) and of
// client/core/src/launch.rs `matching_project_folder`, `prefill_target` and
// `restored_folder`, with the same names where JavaScript allows.
//
// A scoped exception to "rules the core already has are called, never
// rewritten in TypeScript" (plan 2026-10-28-tauri-client-new-session-dialog,
// Task 2 and its rulings): Qt changes the dialog's folder synchronously, and
// its tests depend on the reset and fill in the same turn. The shared golden
// fixture client/core/tests/fixtures/folder-paths.json pins both copies:
// core/tests/folder_paths.rs checks the Rust answers, paths.test.ts these.
//
// Rust walks `char`s (Unicode scalar values); this walks code points
// (`Array.from`), so a character outside the BMP counts once in both.

export type Style = PathStyle;

/**
 * A peer is POSIX. The local computer follows its `metrics.platform`
 * (`windows`, `linux`, `macos`); before it reports one it is the platform
 * this client was built for (`build`, `paths::build` from the dialog's context).
 */
export function of(platform: string | null | undefined, local: boolean, build: Style): Style {
  if (!local) return "posix";
  if (platform === null || platform === undefined || platform === "") return build;
  return platform === "windows" ? "windows" : "posix";
}

const separator = (c: string | undefined) => c === "/" || c === "\\";

/** "X:\" or "X:/". */
const drive = (path: string[]) => path.length >= 3 && path[1] === ":" && separator(path[2]) && /^[A-Za-z]$/.test(path[0]);

/** The end of "\\server" (two separators, then a name), or 0. */
function server(path: string[]): number {
  if (path.length < 3 || !separator(path[0]) || !separator(path[1]) || separator(path[2])) return 0;
  let end = 3;
  while (end < path.length && !separator(path[end])) end += 1;
  return end;
}

/** The end of "\\server\share", or 0 without a share name. */
function share(path: string[]): number {
  const from = server(path) + 1;
  if (from === 1 || from >= path.length || separator(path[from])) return 0;
  let end = from;
  while (end < path.length && !separator(path[end])) end += 1;
  return end;
}

/** "\\?\" and "\\.\" name verbatim paths and devices, not a share. */
const device = (path: string[]) => server(path) === 3 && (path[2] === "?" || path[2] === ".");

const chars = (path: string) => Array.from(path);

const slashes = (path: string) => path.replaceAll("\\", "/");

/**
 * POSIX: a leading '/'. Windows: a drive root ("C:\", "c:/") or a share
 * ("\\server\share", "//server/share"); "\work", "C:work" and "\\?\" paths
 * are not folders of a computer.
 */
export function absolute(path: string, style: Style): boolean {
  if (style === "posix") return path.startsWith("/");
  const text = chars(path);
  return drive(text) || (share(text) > 0 && !device(text));
}

/** Resolves `.` and `..` by names. A `..` beyond the start is kept when `keepParents`, or dropped (a Windows root). */
function names(text: string[], keepParents: boolean): string[] {
  const rest = text.join("");
  const out: string[] = [];
  for (const name of rest.split("/").filter((part) => part !== "")) {
    if (name === ".") continue;
    if (name === ".." && out.length && out[out.length - 1] !== "..") out.pop();
    else if (name === ".." && !keepParents) continue;
    else out.push(name);
  }
  return out;
}

/**
 * The stored form. POSIX: `QDir::cleanPath` (a `..` at the root stays, as Qt
 * keeps it). Windows: '/' separators, no trailing one except in "X:/",
 * "//server/share" keeps its two, and `..` never leaves the root.
 */
export function clean(path: string, style: Style): string {
  if (path === "") return "";
  if (style === "posix") {
    const text = chars(path);
    const rooted = text[0] === "/";
    const rest = names(text, true).join("/");
    if (rooted) return `/${rest}`;
    return rest === "" ? "." : rest;
  }
  const text = chars(slashes(path));
  // Kept as typed: "X:/", "//server/share" (or "//server" without a share), "/".
  let root = drive(text) ? 3 : share(text);
  if (root === 0) root = server(text);
  if (root === 0 && text[0] === "/") root = 1;
  const rest = names(text.slice(root), root === 0).join("/");
  if (root === 0) return rest === "" ? "." : rest;
  const prefix = text.slice(0, root).join("");
  return rest === "" || prefix.endsWith("/") ? prefix + rest : `${prefix}/${rest}`;
}

/** Qt's case-insensitive comparison folds each character on its own; one whose lower case is longer stays as it is. */
function fold(c: string): string {
  const lower = c.toLowerCase();
  return chars(lower).length === 1 ? lower : c;
}

function equalFolded(a: string, b: string): boolean {
  const left = chars(a), right = chars(b);
  return left.length === right.length && left.every((c, index) => fold(c) === fold(right[index]));
}

function startsFolded(text: string, prefix: string): boolean {
  const body = chars(text), head = chars(prefix);
  return head.length <= body.length && head.every((c, index) => fold(c) === fold(body[index]));
}

/** Windows compares with either separator and ignoring case; POSIX compares exactly. */
export function same(a: string, b: string, style: Style): boolean {
  return style === "posix" ? a === b : equalFolded(slashes(a), slashes(b));
}

/** Whether `path` is `folder` or inside it, by whole names: C:/workshop is not in C:/work. */
export function within(path: string, folder: string, style: Style): boolean {
  if (style === "posix") {
    return (folder.startsWith("/") || folder.startsWith(":"))
      && (path === folder || (folder === "/" ? path.startsWith("/") : path.startsWith(folder) && path.charAt(folder.length) === "/"));
  }
  if (!absolute(folder, style)) return false;
  const target = slashes(path);
  const base = slashes(folder);
  const prefix = base.endsWith("/") ? base : `${base}/`;
  return equalFolded(target, base) || startsFolded(target, prefix);
}

/** The last name; empty for a root ("/", "C:/", "//server/share") and after a trailing separator. */
export function leaf(path: string, style: Style): string {
  let text = path;
  if (style === "windows") {
    text = slashes(path);
    const end = share(chars(text));
    if (end > 0 && end === chars(text).length) return "";
  }
  return text.slice(text.lastIndexOf("/") + 1);
}

/** The form shown to the user: backslashes on Windows, whatever the client's platform. */
export function native(path: string, style: Style): string {
  return style === "posix" ? path : path.replaceAll("/", "\\");
}

/**
 * `QFileInfo(path).path()` and `fileName()`, by the folder's style: the
 * parent ("/" for "/repo", "C:/" for "C:/repo") and the last name.
 */
export function split(path: string, style: Style): [string, string] {
  const text = style === "posix" ? path : slashes(path);
  const at = text.lastIndexOf("/");
  if (at < 0) return [".", text];
  // The root keeps its separator: "/" and, on Windows, "X:/".
  const root = at === 0 || (style === "windows" && at === 2 && drive(chars(text)));
  return [root ? text.slice(0, at + 1) : text.slice(0, at), text.slice(at + 1)];
}

/** `QDir(parent).filePath(name)`: one '/' between them. */
export const join = (parent: string, name: string) => (parent.endsWith("/") ? `${parent}${name}` : `${parent}/${name}`);

// launch.rs

const text = (value: unknown, key: string) => string(object(value)[key]);

const flag = (value: unknown, key: string) => object(value)[key] === true;

const entries = (catalog: unknown) => array(object(catalog).worktrees);

/** A catalog whose Git answer is current: `state` ok, not stale, with a common directory. */
const current = (catalog: unknown) => text(catalog, "state") === "ok" && !flag(catalog, "stale") && text(catalog, "common_dir") !== "";

/** The project's folders on `host` ("" is this computer). */
const on = (project: LaunchProject | null | undefined, host: string) =>
  (project?.folders ?? []).filter((folder) => folder.machine === host).map((folder) => folder.path);

/**
 * The selected project's folder on this computer holding `path` or its
 * canonical form, the deepest one; `path` itself when none does.
 */
export function matchingProjectFolder(path: string, canonical: string, project: LaunchProject | null | undefined, host: string, style: Style): string {
  let best = "";
  for (const folder of on(project, host)) {
    const root = clean(folder, style);
    if ((within(path, root, style) || within(canonical, root, style)) && chars(root).length > chars(best).length) best = root;
  }
  return best === "" ? path : best;
}

/**
 * A session's folder that is a linked worktree opens New session on its
 * main checkout (`finishFolderPrefill`): the catalog read for the session's
 * folder must be current and complete, its selected root must hold the
 * folder (or its canonical form) and be listed as linked, and exactly one
 * main checkout must be listed. The answer is the project folder holding
 * that main checkout (or the checkout); the caller keeps `catalog` as the
 * verified catalog of that target and selects it.
 */
export function prefillTarget(catalog: unknown, folder: string, canonical: string, project: LaunchProject | null | undefined, host: string, style: Style): string | null {
  if (!current(catalog) || flag(catalog, "partial")) return null;
  const selected = clean(text(catalog, "selected_root"), style);
  if (!within(folder, selected, style) && !within(canonical, selected, style)) return null;
  let linked = false;
  let main: string | null = null;
  for (const entry of entries(catalog)) {
    const root = clean(text(entry, "path"), style);
    if (!absolute(root, style) || !flag(entry, "available")) continue;
    if (same(root, selected, style) && text(entry, "kind") === "linked") linked = true;
    if (text(entry, "kind") === "main") {
      if (main !== null) return null;
      main = root;
    }
  }
  if (main === null || !linked) return null;
  return matchingProjectFolder(main, "", project, host, style);
}

/**
 * The folder a failed creation's form opens on again, and whether Auto
 * stays chosen (`restoreCreationChoices`): a created worktree or clone
 * opens on its checkout, otherwise the original folder, with Auto only
 * while the worktree was not yet created.
 */
export function restoredFolder(plan: Json): [string, boolean] {
  const worktreeDone = flag(plan, "worktree_done");
  const path = worktreeDone || flag(plan, "clone_done")
    ? text(plan, "target")
    : typeof plan.source === "string" ? plan.source : text(plan, "target");
  return [path, flag(plan, "auto") && !worktreeDone];
}
