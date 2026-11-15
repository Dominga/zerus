import { batch } from "solid-js";
import { page, setPage, type Page } from "../../store/ui";
import { teams } from "../../store/teams";

// A single return destination, shared by the three Catalog entry points.
// Returning to New team uses its existing held create intent and cached catalog.
let origin: { page: Page; newTeam: boolean } | undefined;
export function openTeamCatalog() {
  origin = { page: page() === "team-catalog" ? "teams" : page(), newTeam: teams.newTeamOpen() };
  batch(() => { if (origin!.newTeam) teams.closeNewTeam(); setPage("team-catalog"); });
}
export function closeTeamCatalog() {
  const target = origin ?? { page: "teams", newTeam: false };
  origin = undefined;
  batch(() => { setPage(target.page); if (target.newTeam) void teams.openNewTeam(); });
}
