import { batch } from "solid-js";
import { page, selected, setPage, type Page } from "../../store/ui";
import { teams, type TeamSummary, type TeamView } from "../../store/teams";
import { openTeamLead } from "./lead-navigation";
import { rowIsSession } from "./session-binding";

type TeamLink = Pick<TeamSummary, "id" | "lead_session">;
// One return destination, never a per-session cache. A team link is only a
// navigation hint: Back opens a fresh view with the existing lead-change fence.
let origin: { page: Page; newTeam: boolean; team?: TeamLink } | undefined;
function open(team?: TeamLink) {
  const target = { page: page() === "team-catalog" ? "teams" as const : page(), newTeam: teams.newTeamOpen(), team };
  origin = target;
  batch(() => { if (target.newTeam) teams.closeNewTeam(); setPage("team-catalog"); });
}
export function openTeamCatalog() { open(); }
export function openTeamCatalogFromTeam(view: TeamView) {
  const row = selected(), lead = view.members.find((member) => member.id === view.bar.lead)?.session;
  open(row && lead && rowIsSession(row, lead) ? {
    id: view.bar.id, lead_session: { machine: lead.machine, session_id: lead.session_id },
  } : undefined);
}
export function closeTeamCatalog() {
  const target = origin ?? { page: "teams", newTeam: false };
  origin = undefined;
  batch(() => {
    setPage(target.page);
    const row = selected();
    if (target.page === "sessions" && target.team && row) openTeamLead(target.team, row);
    if (target.newTeam) void teams.openNewTeam();
  });
}
