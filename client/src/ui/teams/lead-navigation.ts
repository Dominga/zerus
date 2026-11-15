import { batch, createEffect, createSignal, onCleanup } from "solid-js";
import { page, selected, setDetailTab, setPage, setSelectedKey } from "../../store/ui";
import { teams, type TeamSession, type TeamSummary } from "../../store/teams";
import type { Session } from "../sessions/types";
import { rowIsSession } from "./session-binding";

// One explicit navigation, not a per-session cache. It keeps a listed team's
// binding while leaving a browser page beyond the automatic lookup's bound.
interface LeadNavigation { key: string; team: string; lead: TeamSession }
const [navigation, setNavigation] = createSignal<LeadNavigation>();

export function navigationTeamFor(row: Session | undefined): string | undefined {
  const target = navigation();
  return row && target?.key === row.key && rowIsSession(row, target.lead) ? target.team : undefined;
}

export function openTeamLead(team: TeamSummary, row: Session): boolean {
  const lead = team.lead_session;
  if (teams.available() !== true || !lead || !rowIsSession(row, lead)) return false;
  batch(() => {
    setNavigation({ key: row.key, team: team.id, lead: { machine: lead.machine, session_id: lead.session_id } });
    setPage("sessions"); setSelectedKey(row.key); setDetailTab("team");
  });
  return true;
}

/** Keep one exact selection binding across its tabs; App owns its lifetime. */
export function watchTeamNavigation() {
  createEffect(() => {
    if (!navigation()) return;
    if (teams.available() !== true || page() !== "sessions" || !navigationTeamFor(selected())) setNavigation(undefined);
  });
  onCleanup(() => setNavigation(undefined));
}
