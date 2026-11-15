import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { orderedRows } from "../../store/catalog";
import { projectsPage } from "../../store/projects";
import { teams, type TeamFailure, type TeamProgress, type TeamSummary } from "../../store/teams";
import { Icon } from "../lib/icons";
import type { Session } from "../sessions/types";
import { openTeamLead } from "../teams/lead-navigation";
import { rowIsSession, rowSessionKeys, sessionKey } from "../teams/session-binding";
import { stateLabel, stateTone } from "../teams/team-model";
import "../styles/teams.css";
import "../styles/teams-screen.css";

const count = (value: number, more: boolean) => `${value}${more ? "+" : ""}`;
const ratio = (done: number, total: number, doneMore: boolean, totalMore: boolean) => `${count(done, doneMore)}/${count(total, totalMore)}`;

function TeamMetrics(props: { progress: TeamProgress }) {
  const p = () => props.progress;
  const tasks = () => ratio(p().accepted, p().tasks, p().accepted_more, p().tasks_more);
  const exact = () => !p().accepted_more && !p().tasks_more;
  return <div class="teams-metrics">
    <span class="teams-task-progress" title={tr("Accepted tasks: %1", tasks())}>
      <span>{tasks()}</span>
      <span class="teams-progress-track" role="progressbar" aria-label={tr("Accepted tasks: %1", tasks())}
        aria-valuemin={0} aria-valuemax={exact() ? p().tasks : undefined} aria-valuenow={exact() ? p().accepted : undefined} aria-valuetext={tasks()}>
        <span style={{ width: `${exact() && p().tasks > 0 ? Math.min(100, p().accepted / p().tasks * 100) : 0}%` }} />
      </span>
    </span>
    <Show when={p().phases > 1 || p().phases_more}><span class="dim">{tr("Phases finished: %1", ratio(p().phases_done, p().phases, p().phases_done_more, p().phases_more))}</span></Show>
    <Show when={p().stuck}><span class="teams-stuck">{tr("Stuck")}</span></Show>
    <Show when={p().questions > 0 || p().questions_more}><span class="teams-questions" title={tr("Unanswered member questions")}>?{count(p().questions, p().questions_more)}</span></Show>
    <span class="teams-member-count">{tr("%1 members", count(p().members, p().members_more))}</span>
  </div>;
}

/** One followed list page at a time; no timer and no per-team reads. */
export function TeamsScreen() {
  const [notice, setNotice] = createSignal("");
  const [pending, setPending] = createSignal<{ cursor: string | null; failure: TeamFailure | null }>();
  const view = () => teams.listPage() ?? teams.list();
  const readFailure = () => teams.readFailureFor("list");
  const first = () => teams.firstListPage().catch((error) => setNotice(String(error)));
  onMount(() => { void first(); });
  onCleanup(() => { void teams.firstListPage().catch(() => {}); });
  createEffect(() => {
    const asked = pending();
    if (asked && ((view()?.after ?? null) === asked.cursor || readFailure() !== asked.failure || teams.available() === false)) setPending(undefined);
  });
  // Keep only the bindings needed by this page, even with a large catalog.
  const leads = createMemo(() => {
    const wanted = new Set((view()?.teams ?? []).map((team) => sessionKey(team.lead_session)).filter(Boolean));
    const found = new Map<string, Session>();
    if (wanted.size) for (const row of orderedRows()) {
      for (const key of rowSessionKeys(row)) if (wanted.has(key) && !found.has(key)) found.set(key, row);
      if (found.size === wanted.size) break;
    }
    return found;
  });
  const project = (id: string) => projectsPage().projects.find((candidate) => candidate.id === id)?.name ?? id;
  const open = (team: TeamSummary) => {
    const row = orderedRows().find((candidate) => rowIsSession(candidate, team.lead_session));
    if (!row || !openTeamLead(team, row)) setNotice(tr("The team's lead session is not on the session list."));
  };
  const move = async (cursor: string | null) => {
    if (pending()) return;
    setNotice(""); setPending({ cursor, failure: readFailure() });
    try { await teams.page("list", cursor); }
    catch (error) { setPending(undefined); setNotice(String(error)); }
  };
  return <section class="teams-screen" aria-label={tr("Teams")} data-teams-page>
    <header class="teams-screen-head">
      <div><h1>{tr("Teams")}</h1><p class="dim">{tr("Teams on this computer")}</p></div>
      <button type="button" class="btn primary" disabled={teams.available() !== true} onClick={() => void teams.openNewTeam()}><Icon name="plus" size={14} />{tr("New team")}</button>
    </header>
    <Show when={teams.available() !== false} fallback={<p class="notice team-notice" role="status">{teams.off() === "unavailable" ? tr("Teams are unavailable on this computer.") : tr("Teams are off on this computer.")}</p>}>
      <Show when={notice()}><p class="notice danger team-notice" role="alert">{notice()}</p></Show>
      <Show when={readFailure()}>{(failure) => <p class="notice danger team-notice" role="alert">{tr("Teams could not be read: %1", failure().message)}</p>}</Show>
      <Show when={pending()}><p class="teams-reading dim" role="status">{tr("Reading teams…")}</p></Show>
      <Show when={view()} fallback={<p class="team-empty dim" role="status">{tr("Reading teams…")}</p>}>
        {(shown) => <>
          <Show when={shown().truncated}><p class="notice team-notice" role="status">{tr("Some teams could not be shown.")}</p></Show>
          <Show when={shown().teams.length} fallback={<div class="teams-empty team-empty" data-teams-empty>
            <Icon name="users" size={28} /><h2>{shown().after || shown().next || shown().truncated ? tr("No teams on this page.") : tr("No teams yet.")}</h2>
            <Show when={!shown().after && !shown().next && !shown().truncated}><p>{tr("Create a team to work on project tasks together.")}</p></Show>
            <button type="button" class="btn primary" onClick={() => void teams.openNewTeam()}>{tr("New team")}</button>
          </div>}>
            <ul class="teams-list">
              <For each={shown().teams}>{(team) => <li>
                <button type="button" class="teams-row" data-team-row={team.id} aria-label={tr("Open team %1", team.name)} onClick={() => open(team)}>
                  <span class="teams-row-head"><Icon name="users" size={18} /><b class="teams-row-name">{team.name}</b><span class={`tm-state-badge tone-${stateTone(team.status)}`}>{stateLabel(team.status)}</span></span>
                  <span class="teams-row-meta"><span title={team.project}><Icon name="folder" size={13} />{project(team.project)}</span><span><span class="dim">{tr("Lead")}</span>{leads().get(sessionKey(team.lead_session))?.name ?? team.lead}</span></span>
                  <Show when={team.progress}>{(progress) => <TeamMetrics progress={progress()} />}</Show>
                </button>
              </li>}</For>
            </ul>
          </Show>
          <nav class="teams-pages" aria-label={tr("Team list pages")}>
            <Show when={shown().after}><button type="button" class="btn small" disabled={!!pending()} onClick={() => void move(null)}>{tr("First page")}</button></Show>
            <Show when={shown().next && shown().next !== shown().after}><button type="button" class="btn small" disabled={!!pending()} onClick={() => void move(shown().next)}>{tr("More teams")}</button></Show>
          </nav>
        </>}
      </Show>
    </Show>
  </section>;
}
