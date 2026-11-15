import { draftKey } from "../../store/draftKeys";
import type { TeamView } from "./types";
import type { Session } from "../sessions/types";

type LeadRow = Pick<Session, "computer" | "catalogKey" | "name" | "sessionId" | "state" | "runId" | "launchId" | "conversationId">;

/** A name or an old composer never establishes the lead. The composer itself
 * keeps the inspected run/conversation and original delivery request UUID. */
export function leadChatBound(view: TeamView, row: LeadRow | undefined, machine: string | null | undefined, composerKey: string | undefined, child: boolean): boolean {
  if (!row || child || row.state === "archived" || !row.runId || !row.conversationId) return false;
  const lead = view.members.find((member) => member.id === view.bar.lead && member.lead);
  const session = lead?.session;
  if (!machine || !session || session.machine !== machine) return false;
  if ((!row.launchId || session.session_id !== `launch_id:${row.launchId}`) && session.session_id !== `run_id:${row.runId}`) return false;
  return composerKey === draftKey({ computer: row.computer, key: row.catalogKey, name: row.name, archive_id: null, session_id: row.sessionId });
}
