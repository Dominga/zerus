// Adapted from WhiteWh's CatalogPage; this page edits our catalog only.
import { Show } from "solid-js";
import { tr } from "../../i18n";
import { setPage } from "../../store/ui";
import { teams } from "../../store/teams";
import { TeamCatalogEditor } from "./TeamCatalogEditor";
export function CatalogPage() {
  return <div class="catalog-page"><header class="nt-page-head"><h1>{tr("Team catalog")}</h1><button class="btn ghost small" onClick={() => setPage("sessions")}>{tr("Back")}</button></header><Show when={teams.available() === true} fallback={<p>{tr("Teams preview is off.")}</p>}><TeamCatalogEditor /></Show></div>;
}
