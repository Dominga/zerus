import { tr } from "../../i18n";
import type { TeamView } from "../../types";

// Task 199 (Qt TeamWorkspace.cpp 3617-3700): the side column's lists, each
// row opening what it names. Since task 345 only Plan problems remains, while
// the team plans; the owner removed Needs your attention and Needs your answer.

export type SideTarget = { task: number } | { member: string } | { phase: number } | null;
export interface SideRow { key: string; role: string; text: string; action: string; target: SideTarget; tone: "warn" | "danger" | "dim" }
export interface SideList { id: "plan"; title: string; note: string; open: boolean; rows: SideRow[] }

const roleOf = (team: Pick<TeamView, "members">, id: string) => team.members.find((member) => member.id === id)?.role || id;

export function sideLists(team: Pick<TeamView, "status" | "members" | "validation">): SideList[] {
  const lists: SideList[] = [];
  // Task 345 (owner, 2026-11-08): no Needs your attention or Needs your
  // answer lists above Members. Each member's card carries its own request
  // (Deny/Allow, the member highlighted), task questions are answered in the
  // Questions list, a draft phase in its plan header, and the wait strip keeps
  // its one counted row.
  // Plan problems: in planning, each finding opens its task.
  if (team.status === "planning") {
    const rows: SideRow[] = [];
    for (const [findings, tone] of [[team.validation.errors, "danger"], [team.validation.warnings, "warn"]] as const) {
      findings.forEach((finding, index) => rows.push({
        key: `${tone}:${index}`, role: finding.member ? roleOf(team, finding.member) : "", text: finding.text,
        action: finding.task ? tr("Open the task") : "", target: finding.task ? { task: finding.task } : null, tone,
      }));
    }
    if (rows.length) {
      const errors = team.validation.errors.length, warnings = team.validation.warnings.length;
      const note = [errors ? (errors === 1 ? tr("1 error") : tr("%1 errors", errors)) : "", warnings ? (warnings === 1 ? tr("1 warning") : tr("%1 warnings", warnings)) : ""].filter(Boolean).join(", ");
      lists.push({ id: "plan", title: tr("Plan problems"), note, open: false, rows });
    }
  }
  return lists;
}
