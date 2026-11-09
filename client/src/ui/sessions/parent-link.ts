import type { TeamSummary } from "../../types";
import type { Session } from "./types";

/**
 * Task 467 (owner, 2026-11-09): hgs records `source: "launch"` for every
 * session started from another session's shell, a plain `hgs launch`
 * included, so a link by itself says only "started by". Such a session stays
 * a top-level card with the hint; it nests only when it really depends on the
 * launcher: the lead of a sub-team of the parent's team (a task the parent
 * handed down). An adopted or person-set link (`adopt`, `manual`) is a
 * dependence on its own, and a record older than the source keeps nesting.
 */
export function dependsOnParent(child: Session, parent: Session | undefined,
  teamOf?: (s: Session) => TeamSummary | undefined): boolean {
  if ((child.parent?.source ?? "") !== "launch") return true;
  if (!parent || !teamOf) return false;
  const team = teamOf(child), up = teamOf(parent);
  return !!team && !!up && !!team.parent && team.parent === up.id;
}
