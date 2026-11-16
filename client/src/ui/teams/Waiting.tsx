import { For, Show } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { openPhase } from "../../store/team-phases";
import { TeamChip } from "./TeamChips";
import type { WaitingItem, WaitingView } from "../../store/teams";
import { waitingAge, waitingDecisions, waitingKindIcon, waitingKindLabel, waitingKindSentence, waitingNote } from "./decisions";

// Waiting for you (the fork's Waiting.tsx, adapted to M1): the open owner
// cards of every team, oldest first, each with its team, its kind and the
// person's one-click decisions. The fork's proposals, permissions, deferred
// requests and report drawers belong to later milestones and are not here.

/** One card: its team, kind, text, age and decisions. */
export function WaitingCard(props: {
  item: WaitingItem;
  teamName: string;
  now: number;
  busy: boolean;
  onDecide: (item: WaitingItem, decision: string) => void;
  onOpenTeam: (item: WaitingItem) => void;
  onOpenTask?: (item: WaitingItem) => void;
}) {
  return (
    <article class="waiting-card" data-waiting={props.item.id} data-kind={props.item.kind}>
      <div class="waiting-card-head">
        <span class="team-stage-chip waiting-kind"><Icon name={waitingKindIcon(props.item.kind)} size={12} /><b>{waitingKindLabel(props.item)}</b></span>
        <button type="button" class="waiting-team btn small" title={tr("Open the team")} onClick={() => props.onOpenTeam(props.item)}>
          <TeamChip name={props.teamName || props.item.team} />
        </button>
        <span class="waiting-age dim">{waitingAge(props.item.created_ms, props.now)}</span>
      </div>
      <p class="waiting-kind-sentence dim" data-kind-sentence>{waitingKindSentence(props.item)}</p>
      <p class="waiting-text">{props.item.text}</p>
      <Show when={(props.item.notes ?? []).length > 0}>
        <div class="waiting-notes" data-notes>
          <p class="waiting-notes-heading dim">{tr("Service notes")}</p>
          <For each={props.item.notes}>
            {(note) => <p class="waiting-note" data-note={note.code}>{waitingNote(note)}</p>}
          </For>
        </div>
      </Show>
      <div class="waiting-decisions">
        <For each={waitingDecisions(props.item)}>
          {(choice, index) => (
            <button type="button" class="btn small" classList={{ danger: choice.danger, primary: index() === 0 && !choice.danger }}
              disabled={props.busy} data-decision={choice.decision}
              onClick={() => props.onDecide(props.item, choice.decision)}>{choice.text}</button>
          )}
        </For>
        <Show when={props.item.phase_reference}>{(reference) => <button type="button" class="btn small" disabled={props.busy} data-open-phase onClick={() => { openPhase(props.item.team,reference().phase); props.onOpenTeam(props.item); }}>{tr("Open phase")}</button>}</Show>
        <Show when={props.item.node !== null}>
          <button type="button" class="btn small" disabled={props.busy} data-open-task
            onClick={() => props.onOpenTask?.(props.item)}>{tr("Open task")}</button>
        </Show>
      </div>
    </article>
  );
}

/** The page's list, paged by its `next` cursor; a later page has the way back
 * to the first and never says that nothing waits. */
export function WaitingList(props: {
  view: WaitingView;
  teamNames: ReadonlyMap<string, string>;
  now: number;
  busy: boolean;
  onDecide: (item: WaitingItem, decision: string) => void;
  onOpenTeam: (item: WaitingItem) => void;
  onOpenTask?: (item: WaitingItem) => void;
  onPage: (cursor: string) => void;
  onFirst?: () => void;
}) {
  return (
    <section class="waiting-list" aria-label={tr("Waiting for you")} data-waiting-list>
      <Show when={props.view.items.length} fallback={
        <Show when={props.view.after} fallback={
          <div class="empty team-empty"><Icon name="list-todo" size={24} /><h3>{tr("Nothing waits for you.")}</h3><p>{tr("Decisions that only you can make appear here.")}</p></div>
        }>
          <div class="empty team-empty" data-waiting-later-empty><Icon name="list-todo" size={24} /><h3>{tr("No open cards on this page.")}</h3><p>{tr("Earlier pages can still have open cards.")}</p></div>
        </Show>
      }>
        <For each={props.view.items}>
          {(item) => <WaitingCard item={item} teamName={props.teamNames.get(item.team) ?? ""} now={props.now} busy={props.busy}
            onDecide={props.onDecide} onOpenTeam={props.onOpenTeam} onOpenTask={props.onOpenTask} />}
        </For>
      </Show>
      <Show when={props.view.after || props.view.next}>
        <div class="waiting-pages">
          <Show when={props.view.after}>
            <button type="button" class="btn small" disabled={props.busy} data-waiting-first onClick={() => props.onFirst?.()}>{tr("First page")}</button>
          </Show>
          <Show when={props.view.next}>
            {(cursor) => (
              <button type="button" class="btn small" disabled={props.busy} onClick={() => props.onPage(cursor())}>{tr("More…")}</button>
            )}
          </Show>
        </div>
      </Show>
    </section>
  );
}
