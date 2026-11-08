import { tr } from "../../i18n";
import type { TeamView } from "../../types";

// Task 199 (Qt TeamWorkspace.cpp 3617-3700): the side column's lists, each
// row opening what it names: Needs your attention (what the lead left to the
// owner), Needs your answer (a question in a session, a task's question, the
// first draft phase) and, while the team plans, Plan problems.

export type SideTarget = { task: number } | { member: string } | { phase: number } | null;
export interface SideRow { key: string; role: string; text: string; action: string; target: SideTarget; tone: "warn" | "danger" | "dim" }
export interface SideList { id: "attention" | "answers" | "plan"; title: string; note: string; open: boolean; rows: SideRow[] }

const roleOf = (team: Pick<TeamView, "members">, id: string) => team.members.find((member) => member.id === id)?.role || id;
const record = (value: unknown): Record<string, unknown> => (value && typeof value === "object" ? value as Record<string, unknown> : {});
const str = (value: unknown) => (typeof value === "string" ? value : "");

export function sideLists(team: Pick<TeamView, "status" | "lead" | "members" | "tasks" | "phases" | "attention" | "validation">): SideList[] {
  const lists: SideList[] = [];
  // 0. Needs your attention (owner, 2026-10-28): open by default.
  const attention = (team.attention ?? []).slice(0, 50).map((item, index): SideRow => {
    const fields = record(item);
    const task = Number(fields.task) || 0;
    const member = str(fields.member);
    return {
      key: `attention:${index}`,
      role: member ? roleOf(team, member) : "",
      text: str(fields.summary) || str(fields.text) || str(fields.title) || str(fields.kind),
      action: task ? tr("Open the task") : "",
      target: task ? { task } : null,
      tone: "warn",
    };
  });
  if (attention.length) lists.push({ id: "attention", title: tr("Needs your attention"), note: String(attention.length), open: true, rows: attention });
  // 1. Needs your answer: questions in sessions, task questions, a phase draft.
  const answers: SideRow[] = [];
  for (const member of team.members) {
    if (!member.session || !member.question) continue;
    answers.push({ key: `question:${member.id}`, role: roleOf(team, member.id), text: tr("Question"), action: tr("Go to question"), target: { member: member.id }, tone: "warn" });
  }
  for (const task of team.tasks) for (const asked of task.questions ?? []) {
    answers.push({ key: `task:${task.id}:${asked.id}`, role: roleOf(team, task.assignee), text: `#${task.id} ${task.title} — ${asked.text}`, action: tr("Answer"), target: { task: task.id }, tone: "warn" });
  }
  if (team.status !== "planning") {
    const draft = team.phases.find((phase) => phase.state === "draft");
    if (draft) answers.push({ key: `phase:${draft.n}`, role: roleOf(team, team.lead), text: tr("Phase %1: %2, draft waits for your Accept", draft.n, draft.title), action: tr("Review"), target: { phase: draft.n }, tone: "warn" });
  }
  if (answers.length) lists.push({ id: "answers", title: tr("Needs your answer"), note: String(answers.length), open: false, rows: answers });
  // 3. Plan problems: in planning, each finding opens its task.
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
