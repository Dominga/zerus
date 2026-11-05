import { createMemo, onCleanup, onMount, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { teamAction } from "../../bridge";
import { createNewTeam } from "../../store/teamsNew";
import { selected, setPage } from "../../store/ui";
import { catalog as sessions, computerFresh } from "../../store/catalog";
import { accountsPage, loadAccounts } from "../../store/accounts";
import { showCatalog, teams } from "../../store/teams";
import { accountChoices, catalogOf, computerChoices, coordinatorOf } from "../../store/newTeamSources";
import { NewTeam } from "./NewTeam";

/** New team (Qt's NewTeamWindow): opening creates nothing; Start planning
 * creates the draft and its lead session. Task 56: it offers what New team
 * offers anywhere: every computer, this computer's accounts, the catalog's
 * roles and templates, and the selected session as "This session". */
export function NewTeamPage(): JSX.Element {
  const store = createNewTeam({ action: (call) => teamAction(call) });
  onMount(() => { store.openNew(); void loadAccounts(); });
  // The catalog is read while the form shows.
  onCleanup(showCatalog(""));
  const computers = createMemo(() => computerChoices(sessions.computers, accountsPage().hosts ?? [], (computer) => computer === "" || computerFresh(computer)));
  const accounts = createMemo(() => accountChoices(accountsPage()));
  const catalog = createMemo(() => catalogOf(teams.catalog("")));
  const coordinator = createMemo(() => coordinatorOf(selected()));
  return (
    <div class="newteam-page">
      <header class="nt-page-head">
        <h1>{tr("New team")}</h1>
        <button type="button" class="btn ghost small" onClick={() => setPage("sessions")}>{tr("Back to sessions")}</button>
      </header>
      <NewTeam store={store} computer="" computers={computers()} accounts={accounts()} catalog={catalog()} coordinator={coordinator()}
        git={true} onOpenSettings={() => {}} onCreated={() => setPage("sessions")} />
    </div>
  );
}
