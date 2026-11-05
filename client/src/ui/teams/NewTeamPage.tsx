import { createEffect, onCleanup, onMount, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { teamAction } from "../../bridge";
import { createNewTeam } from "../../store/teamsNew";
import { showTeam, teams } from "../../store/teams";
import { setPage } from "../../store/ui";
import { NewTeam } from "./NewTeam";
import type { DraftMember } from "./types";

/** New team (Qt's NewTeamWindow): opening creates nothing; Start planning
 * creates the draft and its lead session. */
export function NewTeamPage(): JSX.Element {
  const store = createNewTeam({ action: teamAction });
  onMount(() => store.openNew());
  createEffect(() => {
    const id = store.draftId();
    if (!id) return;
    const hide = showTeam("", id);
    onCleanup(hide);
  });
  createEffect(() => {
    const id = store.draftId();
    if (!id) return;
    const view = teams.newTeam("", id);
    if (view) store.applyView(view);
  });
  const lead: DraftMember = {
    id: "lead",
    role: "Lead",
    agent: "dsh",
    model: "",
    effort: "",
    account: "",
    permissions: "",
    computer: "",
    folder: "",
    prompt: "",
    lead: true,
    session: null,
  };
  return (
    <div class="newteam-page">
      <header class="nt-page-head">
        <h1>{tr("New team")}</h1>
        <button type="button" class="btn ghost small" onClick={() => setPage("sessions")}>{tr("Back to sessions")}</button>
      </header>
      <NewTeam store={store} computers={[]} git={true} lead={lead} onCreated={() => setPage("sessions")} />
    </div>
  );
}
