// Adapted from WhiteWh's proposal card presentation. Our cards carry a bounded
// list of decision words, not editable fork-engine proposals or permissions.
import { For, Show } from "solid-js";
import { tr } from "../../i18n";
import type { DrawerRow } from "../../store/teamDrawers";
import type { DrawerAction } from "./TeamReports";
import { waitingDecisions, waitingKindLabel, waitingKindSentence, waitingNote } from "./decisions";

export function ProposalCards(props: { rows: DrawerRow[]; busy: boolean; onAction: DrawerAction }) {
  return <div class="team-proposals" role="list" aria-label={tr("Cards")}>
    <For each={props.rows} fallback={<p class="dim">{tr("No cards on this page.")}</p>}>{(row) =>
      <Show when={row.card}>{(card) => <article class="team-proposal" data-proposal={card().card}>
        <header><b>{waitingKindLabel(card())}</b><span class="dim">{card().by}</span><span class="dim">{card().state}</span></header>
        <p class="dim">{waitingKindSentence(card())}</p>
        <pre class="team-drawer-text">{card().text}</pre>
        <Show when={row.text_truncated}><p class="dim">{tr("Only the beginning of this text is shown.")}</p></Show>
        <Show when={(card().notes ?? []).length}><p class="dim">{tr("Service notes")}</p>
          <For each={card().notes}>{(note) => <p class="dim">{waitingNote(note)}</p>}</For>
        </Show>
        <Show when={card().state === "open"}><div class="team-drawer-actions">
          <For each={waitingDecisions(card())}>{(choice) => <button type="button" class="btn small" classList={{ danger: choice.danger }} disabled={props.busy} data-card-decision={choice.decision}
            onClick={() => props.onAction("decide", { card: card().card, decision: choice.decision })}>{choice.text}</button>}</For>
        </div></Show>
      </article>}</Show>
    }</For>
  </div>;
}
