import { createEffect, createMemo, createRoot } from "solid-js";
import { teamsFollow } from "../bridge";
import { catalog } from "./catalog";
import { page, selected } from "./ui";
import { NOTHING, createTeams, sessionIs } from "./teamsView";
import type { ComputerId, TeamMemberView } from "../types";
import type { Session } from "../ui/sessions/types";

// The window's Teams views (store/teamsView.ts) and what it follows: the team
// list of the selected session's computer, to find its team, and that team.
export { sessionIs, type TeamsEntry, type TeamsFollow, type TeamsStore } from "./teamsView";

// The bridge is read when a follow is said, not when the module loads.
export const teams = createTeams({ follow: (computer, follow) => teamsFollow(computer, follow) });

/** The selected session's team, from its computer's team list. */
export const selectedTeam = createRoot(() => createMemo(() => {
  const row = selected();
  return row && row.state !== "archived" ? teams.teamOf(row) : undefined;
}));

/** The catalog row of a member's session, if the session list has it. */
export function memberRow(teamComputer: ComputerId, member: Pick<TeamMemberView, "session"> | undefined): Session | undefined {
  const session = member?.session;
  if (!session) return undefined;
  for (const id of catalog.order) {
    const row = catalog.rows[id];
    if (row && row.state !== "archived" && sessionIs(session, teamComputer, row)) return row;
  }
  return undefined;
}

// The Sessions page reads the selected session's computer: its team list, and
// the team the session belongs to. Another page, or no selection, reads nothing.
createRoot(() => {
  let current: ComputerId | undefined;
  createEffect(() => {
    const row = page() === "sessions" ? selected() : undefined;
    const computer = row && row.state !== "archived" ? row.computer : undefined;
    if (current !== undefined && current !== computer) teams.follow(current, NOTHING);
    current = computer;
    if (computer === undefined) return;
    teams.follow(computer, { list: true, waiting: false, team: selectedTeam()?.id ?? null });
  });
});
