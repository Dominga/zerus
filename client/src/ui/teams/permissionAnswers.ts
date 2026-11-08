import { tr } from "../../i18n";
import type { TeamPermissionAnswer } from "../../types";

// Task 315 (F9): a member's answered permission requests on its card, as Qt's
// TeamWidgets.h draws them (U17: permissionSummary, permissionAnswerRow,
// permissionAnswerList). Qt separates the parts with middle dots; the UI never
// separates values with dots, so commas stand here.

/** Qt's agoText: "just now", "40 s ago", "3 min ago", "2 h ago", "5 d ago". */
export function agoText(then: number, now: number): string {
  if (then <= 0 || now <= then + 1) return tr("just now");
  const seconds = Math.trunc(now - then);
  if (seconds < 60) return tr("%1 s ago", seconds);
  if (seconds < 3600) return tr("%1 min ago", Math.trunc(seconds / 60));
  if (seconds < 86400) return tr("%1 h ago", Math.trunc(seconds / 3600));
  return tr("%1 d ago", Math.trunc(seconds / 86400));
}

/** Qt's permissionBy: who answered. */
export function permissionBy(by: string): string {
  switch (by) {
    case "owner": return tr("you");
    case "lead": return tr("the lead");
    case "rule": return tr("a rule");
    case "timeout": return tr("a timeout");
    default: return tr("unknown");
  }
}

const denied = (answer: TeamPermissionAnswer) => answer.decision === "deny";

/** "Permissions: 18 allowed, 2 denied, last: allowed by the lead, 3 min ago"; empty without answers. */
export function permissionSummary(answers: readonly TeamPermissionAnswer[], now: number): string {
  if (!answers.length) return "";
  const no = answers.filter(denied).length;
  const last = answers[answers.length - 1];
  const decision = denied(last) ? tr("denied by %1", permissionBy(last.by)) : tr("allowed by %1", permissionBy(last.by));
  return tr("Permissions: %1 allowed, %2 denied, last: %3, %4", answers.length - no, no, decision, agoText(last.at, now));
}

export interface AnswerRow {
  decision: string;
  denied: boolean;
  command: string;
  reason: string;
  tip: string;
}

/** One answer as its row shows it: the decision and its age, the command (or the tool) and the deny text. */
export function answerRow(answer: TeamPermissionAnswer, now: number): AnswerRow {
  const decision = tr("%1, %2", denied(answer) ? tr("Denied by %1", permissionBy(answer.by)) : tr("Allowed by %1", permissionBy(answer.by)), agoText(answer.at, now));
  const command = answer.command || answer.tool;
  return { decision, denied: denied(answer), command, reason: answer.reason, tip: [decision, command, answer.reason].filter(Boolean).join("\n") };
}

/** The newest `limit` answers, newest first, and how many more there are. */
export function newestAnswers(answers: readonly TeamPermissionAnswer[], limit = 5): { shown: TeamPermissionAnswer[]; more: number } {
  const shown = answers.slice(-limit).reverse();
  return { shown, more: answers.length - shown.length };
}
