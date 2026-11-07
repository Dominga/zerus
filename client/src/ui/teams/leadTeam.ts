import { sessionIs } from "../../store/teamsView";
import type { TeamList, TeamSummary, TeamSessionView } from "../../types";
import type { Session } from "../sessions/types";

// Qt U57 (tray/src/SessionsWindow.cpp:7616-7626): the selected session's own
// proposals are the cards above its composer. Qt looks the session up in the
// polled team list and shows the team when the session is its lead; a
// delegated sub-team's lead session is also a member of the parent team it was
// handed the task by, so the team it leads comes before the team it merely
// belongs to.

/** What a team read says about its lead and members (a list entry or a full team). */
interface LeadMember { id: string; lead: boolean; session: TeamSessionView | null }
interface LedTeam { lead: string; members: readonly LeadMember[] }

/** Whether the session is this team's lead: the lead flag or the lead's id names it. */
export function leadsTeam(team: LedTeam, row: Pick<Session, "computer" | "name">): boolean {
  return team.members.some((member) => (member.lead || member.id === team.lead) && sessionIs(member.session, row.computer, row));
}

/** The team a session leads, from the polled list; undefined for a member's session. */
export function ledTeamOf(list: TeamList | undefined, row: Pick<Session, "computer" | "name"> | undefined): TeamSummary | undefined {
  if (!row) return undefined;
  return list?.teams.find((team) => team.status !== "archived" && leadsTeam(team, row));
}

/** The "?" of the Sessions Team tab: what waits for the owner, open proposals included (Qt U57). */
export const teamAnswers = (team: Pick<TeamSummary, "attention" | "open_proposals"> | undefined): number =>
  (team?.attention ?? 0) + (team?.open_proposals ?? 0);
