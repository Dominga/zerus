import { tr } from "../../i18n";
import { orderedRows } from "../../store/catalog";
import { composer, draftKey } from "../../store/composer";
import { setDetailTab, setSelectedKey } from "../../store/ui";
import type { ComputerId, TeamInboxItem, TeamMemberView, TeamTaskView, TeamView } from "../../types";

// Task #415: a task whose attempt ended without a report shows the member's
// last reply (hgs keeps it on the attempt and carries it in the no_report
// inbox item) with the owner's choices: use it as the report, remind the
// member, or retry. Tauri has no direct message to a closed session, so
// Remind opens the member's session with the reminder as its draft.

/** The member's last reply a No report attempt kept; "" when there is none. */
export function noReportReply(items: readonly TeamInboxItem[], task: number): string {
  const item = items.find((found) => found.kind === "no_report" && found.task === task);
  return typeof item?.detail === "string" ? item.detail.trim() : "";
}

/** The reminder Qt's TeamPanel::remind sends: the report command with its JSON body. */
export function reminderText(team: TeamView, task: TeamTaskView, member: TeamMemberView): string {
  const host = member.session?.host ? `@${member.session.host} ` : "";
  const report = JSON.stringify({ request_id: uuid(), team: team.id, task: task.id, attempt: task.attempt_id ?? "", text: "..." });
  return [
    tr("[Zerus team, from: You, task: #%1, attempt: %2]", task.id, task.attempts),
    tr("Your turn ended without a report for task #%1 %2. Report once with JSON on stdin:", task.id, task.title),
    `  hgs ${host}team report --json`,
    `  ${report}`,
  ].join("\n");
}

/**
 * Opens the member's session with the reminder as its draft. `missing`: the
 * member's session is not listed; `draft`: the session already had a draft.
 */
export async function remindMember(computer: ComputerId, team: TeamView, task: TeamTaskView): Promise<"offered" | "draft" | "missing"> {
  const member = team.members.find((found) => found.id === task.assignee);
  const session = member?.session;
  if (!member || !session) return "missing";
  const row = orderedRows().find((found) => found.name === session.name && String(found.computer) === String(computer));
  if (!row) return "missing";
  const key = draftKey({
    computer: row.computer, key: row.catalogKey, name: row.name,
    archive_id: row.state === "archived" ? row.archiveId ?? null : null, session_id: row.sessionId ?? null,
  });
  setSelectedKey(row.key);
  setDetailTab("activity");
  return (await composer.offer(key, reminderText(team, task, member))) ? "offered" : "draft";
}

function uuid(): string {
  const crypto = globalThis.crypto;
  return crypto && typeof crypto.randomUUID === "function" ? crypto.randomUUID() : "00000000-0000-4000-8000-000000000000";
}
