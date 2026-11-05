import type { JSX } from "solid-js";
import { tr } from "../../../i18n";
import { setPage } from "../../../store/ui";

/** Settings > Teams: the ways Qt reaches the Teams screens. */
export function TeamsSection(): JSX.Element {
  return (
    <div class="set-section">
      <h2>{tr("Teams")}</h2>
      <p class="set-note">{tr("New team, the plan's settings and the team catalog are on the team's own bar; the catalog is also here.")}</p>
      <div class="set-actions">
        <button type="button" class="btn small" onClick={() => setPage("newteam")}>{tr("New team")}</button>
        <button type="button" class="btn ghost small" onClick={() => setPage("catalog")}>{tr("Team catalog")}</button>
        <button type="button" class="btn ghost small" onClick={() => setPage("sessions")}>{tr("Open the Team tab")}</button>
      </div>
    </div>
  );
}
