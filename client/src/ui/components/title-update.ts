import type { UpdatesView } from "../../types";

/**
 * The title bar's update button beside the version (owner, 2026-11-13: "the
 * update button is at the top right, together with the release number"; it
 * used to sit at the bottom right of the status bar). A fork build follows the
 * fork's feed: a published newer build, a replacement offered to an unnumbered
 * build, or a verified build waiting for Restart. Any other build follows
 * upstream's channel, unless its prompts are quiet.
 */
export type TitleUpdate =
  | { kind: "none" }
  | { kind: "available"; version?: string; build?: number }
  | { kind: "restart"; build?: number };

export function titleUpdate(view: UpdatesView): TitleUpdate {
  const fork = view.fork;
  if (fork) {
    if (fork.ready_build > 0) return { kind: "restart", build: fork.ready_build };
    const newer = fork.newer_build ?? 0;
    if (newer > 0) return { kind: "available", build: newer };
    if (fork.offered_build > 0) return { kind: "available", build: fork.offered_build };
  }
  if (view.quiet) return { kind: "none" };
  if (view.restart_required) return { kind: "restart" };
  if (view.available) return { kind: "available", version: view.version };
  return { kind: "none" };
}
