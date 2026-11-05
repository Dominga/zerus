import { onMount, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { teamAction } from "../../bridge";
import { createNewTeam } from "../../store/teamsNew";
import { setPage } from "../../store/ui";
import { NewTeam } from "./NewTeam";

/** New team (Qt's NewTeamWindow): opening creates nothing; Start planning
 * creates the draft and its lead session. */
export function NewTeamPage(): JSX.Element {
  const store = createNewTeam({ action: teamAction });
  onMount(() => store.openNew());
  return (
    <div class="newteam-page">
      <header class="nt-page-head">
        <h1>{tr("New team")}</h1>
        <button type="button" class="btn ghost small" onClick={() => setPage("sessions")}>{tr("Back to sessions")}</button>
      </header>
      <NewTeam store={store} computer="" computers={[]} git={true} onCreated={() => setPage("sessions")} />
    </div>
  );
}
