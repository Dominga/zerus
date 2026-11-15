import { Show, children, type JSX } from "solid-js";
import { tr } from "../../i18n";
import type { TeamSummary } from "../../store/teams";

/** Header actions (e.g. Catalog) share a wrapping group with New team. */
export function TeamsHeader(props: { actions?: JSX.Element; children: JSX.Element }) {
  return <header class="teams-screen-head">
    <div class="teams-screen-title"><h1>{tr("Teams")}</h1><p class="dim">{tr("Teams on this computer")}</p></div>
    <div class="teams-screen-actions">{props.actions}{props.children}</div>
  </header>;
}

/** Row actions and an optional drawer are siblings of the lead-open button. */
export function TeamsRow(props: { team: TeamSummary; onOpen: () => void; actions?: JSX.Element; details?: JSX.Element; children: JSX.Element }) {
  const actions = children(() => props.actions), details = children(() => props.details);
  return <li class="teams-row-item" data-team-id={props.team.id}>
    <div class="teams-row">
      <button type="button" class="teams-row-open" data-team-row={props.team.id} aria-label={tr("Open team %1", props.team.name)} onClick={props.onOpen}>
        {props.children}
      </button>
      <Show when={actions()}>{(content) => <div class="teams-row-actions">{content()}</div>}</Show>
    </div>
    <Show when={details()}>{(content) => <div class="teams-row-details">{content()}</div>}</Show>
  </li>;
}
