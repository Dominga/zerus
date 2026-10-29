import type { PathStyle } from "../../types";

// The folder rules the start-up chunk needs for the Projects entries
// (folder-launch.ts): a computer's folder style, whether a path is a folder
// of a computer, and the form shown to the user. Part of the port of
// client/core/src/paths.rs in paths.ts, which re-exports them; the shared
// golden fixture pins them there (paths.test.ts). The root helpers are
// exported for paths.ts only.

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
export const drive = (path: string[]) => path.length >= 3 && path[1] === ":" && separator(path[2]) && /^[A-Za-z]$/.test(path[0]);

/** The end of "\\server" (two separators, then a name), or 0. */
export function server(path: string[]): number {
  if (path.length < 3 || !separator(path[0]) || !separator(path[1]) || separator(path[2])) return 0;
  let end = 3;
  while (end < path.length && !separator(path[end])) end += 1;
  return end;
}

/** The end of "\\server\share", or 0 without a share name. */
export function share(path: string[]): number {
  const from = server(path) + 1;
  if (from === 1 || from >= path.length || separator(path[from])) return 0;
  let end = from;
  while (end < path.length && !separator(path[end])) end += 1;
  return end;
}

/** "\\?\" and "\\.\" name verbatim paths and devices, not a share. */
const device = (path: string[]) => server(path) === 3 && (path[2] === "?" || path[2] === ".");

export const chars = (path: string) => Array.from(path);

export const slashes = (path: string) => path.replaceAll("\\", "/");

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

/** The form shown to the user: backslashes on Windows, whatever the client's platform. */
export function native(path: string, style: Style): string {
  return style === "posix" ? path : path.replaceAll("/", "\\");
}
