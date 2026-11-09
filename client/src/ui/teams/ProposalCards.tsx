import { For, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { tr } from "../../i18n";
import { teamAction, teamRead } from "../../bridge";
import { teamConfirm } from "../../store/teamConfirm";
import { showTeam, teams } from "../../store/teams";
import { selected, subagentOpen } from "../../store/ui";
import { ProposalCards, proposalKindLabel, type CardState, type DecisionPort } from "./decisions";
import { ledTeamOf, leadsTeam } from "./leadTeam";
import type { ComputerId, TeamView } from "../../types";
import { TextInput } from "../lib/TextField";

// The lead's proposals as cards (Qt TeamProposalCards, task 11): the kind in
// words, what changes, who asks and why; Deny with a reason the lead gets,
// Allow; a question's options with the recommended one marked, or the owner's
// words. A card that says something (a refusal, a change meanwhile, an earlier
// decision) keeps saying it; a decided one stays until dismissed.

/** The window's port: the core's team actions, the confirmation dialog and a one-shot read. */
export const windowPort: DecisionPort = {
  send: (call) => teamAction(call),
  confirm: (confirmation) => teamConfirm.ask(confirmation),
  choose: (choice) => teamConfirm.askChoice(choice),
  read: (computer, team) => teamRead(computer, team),
};

/** A model whose changes redraw what reads `tick`. */
export function useModel<T>(make: (changed: () => void) => T): { model: T; tick: () => number } {
  const [tick, setTick] = createSignal(0);
  const model = make(() => setTick((n) => n + 1));
  return { model, tick };
}

const askedAt = (at: number) => {
  if (at <= 0) return "";
  const when = new Date(at * 1000);
  const today = new Date().toDateString() === when.toDateString();
  return today ? when.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : when.toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
};

/** One card. */
export function ProposalCardView(props: { cards: ProposalCards; id: string; tick: () => number; onAnswered?: () => void }) {
  const found = () => (props.tick(), props.cards.cards().find((state) => state.card.id === props.id));
  return <Show when={found()}>{(state) => <Card cards={props.cards} state={state()} onAnswered={props.onAnswered} />}</Show>;
}

function Card(props: { cards: ProposalCards; state: CardState; onAnswered?: () => void }) {
  const card = () => props.state.card;
  const when = () => askedAt(card().at);
  const off = () => props.state.sending;
  const [reasonOpen, setReasonOpen] = createSignal(false);
  const reasonText = () => card().reason ? tr("%1: %2", card().by, card().reason) : "";
  // A reason that can outgrow a few lines gets More; the CSS clamps it.
  const longReason = () => reasonText().length > 160 || reasonText().split("\n").length > 3;
  const answer = async (allow: boolean) => {
    const outcome = await props.cards.answer(card().id, allow);
    if (outcome && outcome !== "cancelled" && outcome !== "stale" && outcome.outcome === "done") props.onAnswered?.();
  };
  const answerQuestion = async () => {
    const outcome = await props.cards.answerQuestion(card().id);
    if (outcome && outcome !== "cancelled" && outcome !== "stale" && outcome.outcome === "done") props.onAnswered?.();
  };
  const ready = () => (props.state.own ? props.state.reason.trim() !== "" : props.state.picked !== "");
  return (
    <div class="team-proposal" classList={{ stale: props.state.gone }} data-proposal={card().id} data-kind={card().kind} data-hash={card().hash}>
      <div class="team-proposal-head">
        <span class="team-pill warn" title={proposalKindLabel(card().kind)}>{proposalKindLabel(card().kind)}</span>
        <b class="team-proposal-what">{card().what}</b>
      </div>
      <div class="team-proposal-by dim">{when() ? tr("%1 asks, %2", card().by, when()) : tr("%1 asks", card().by)}</div>
      <For each={card().lines}>{(line) => <div class="team-proposal-change">{line}</div>}</For>
      <Show when={card().command}><code class="team-proposal-command" data-proposal-command>{card().command}</code></Show>
      <Show when={card().folder}><div class="team-proposal-change" data-proposal-folder>{tr("Folder: %1", card().folder)}</div></Show>
      <Show when={card().reason}>
        <div class="team-proposal-reason" classList={{ clamped: !reasonOpen() }}>{reasonText()}</div>
        <Show when={longReason()}><button type="button" class="link team-proposal-more" onClick={() => setReasonOpen((open) => !open)}>{reasonOpen() ? tr("Show less") : tr("Show more")}</button></Show>
      </Show>
      <Show when={props.state.notice}><div class="team-proposal-notice" role="status">{props.state.notice}</div></Show>
      <Show when={!props.state.gone} fallback={
        <div class="team-proposal-answers"><span class="grow" /><button class="btn small ghost" onClick={() => props.cards.dismiss(card().id)}>{tr("Dismiss")}</button></div>
      }>
        <Show when={card().kind === "question"} fallback={
          <div class="team-proposal-answers">
            <TextInput class="field team-proposal-reason-edit" maxlength={2000} placeholder={tr("Why deny? The lead receives this")} disabled={off()}
              value={props.state.reason} onInput={(event) => props.cards.setReason(card().id, event.currentTarget.value)}
              onKeyDown={(event) => { if (event.key === "Enter" && event.currentTarget.value.trim()) void answer(false); }} />
            <button class="btn small danger" disabled={off()} onClick={() => void answer(false)}>{tr("Deny")}</button>
            <button class="btn small primary" disabled={off()} onClick={() => void answer(true)}>{tr("Allow")}</button>
          </div>
        }>
          <div class="team-proposal-question" role="radiogroup" aria-label={card().what}>
            <For each={card().options}>{(option) => (
              <label class="team-proposal-option">
                <input type="radio" name={`proposal-${card().id}`} disabled={off()} checked={!props.state.own && props.state.picked === option}
                  onChange={() => props.cards.pick(card().id, option)} />
                {option === card().recommended ? tr("%1 (recommended)", option) : option}
              </label>
            )}</For>
            <label class="team-proposal-option">
              <input type="radio" name={`proposal-${card().id}`} disabled={off()} checked={props.state.own} onChange={() => props.cards.chooseOwnWords(card().id)} />
              {card().options.length ? tr("Other") : tr("Your answer")}
            </label>
          </div>
          <div class="team-proposal-answers">
            <Show when={props.state.own} fallback={<span class="grow" />}>
              <TextInput class="field team-proposal-reason-edit" maxlength={4000} placeholder={tr("Your answer, for the lead")} disabled={off()}
                value={props.state.reason} onInput={(event) => props.cards.setReason(card().id, event.currentTarget.value)}
                onKeyDown={(event) => { if (event.key === "Enter") void answerQuestion(); }} />
            </Show>
            <button class="btn small primary" disabled={off() || !ready()} onClick={() => void answerQuestion()}>{tr("Answer")}</button>
          </div>
        </Show>
      </Show>
    </div>
  );
}

/** Every card of a model. */
export function ProposalCardList(props: { cards: ProposalCards; tick: () => number; onAnswered?: () => void }) {
  const ids = () => (props.tick(), props.cards.cards().map((state) => state.card.id));
  return (
    <Show when={ids().length > 0}>
      <div class="team-proposals" role="list" aria-label={tr("Proposals")}>
        <For each={ids()}>{(id) => <ProposalCardView cards={props.cards} id={id} tick={props.tick} onAnswered={props.onAnswered} />}</For>
      </div>
    </Show>
  );
}

/** The cards of the team shown in the Team tab, in the lead's conversation. */
export function TeamProposals(props: { computer: ComputerId; team: TeamView }) {
  const { model, tick } = useModel((changed) => new ProposalCards(windowPort, { computer: props.computer, teamName: () => props.team.name },
    () => teams.team(props.computer, props.team.id) ?? props.team, changed));
  // The first team paints with the render (SSR and the first client frame); the
  // effect takes every later read.
  model.setTeam(props.team);
  createEffect(() => model.setTeam(props.team));
  onCleanup(() => model.setTeam(undefined));
  return <ProposalCardList cards={model} tick={tick} />;
}

/**
 * The selected session's own proposals in its Activity (Qt U57
 * mainProposalCards, SessionsWindow.cpp:7616-7626): the open cards of the team
 * it leads, above the question card and the composer. A member's session shows
 * none; a sub-lead shows its sub-team, which is not the team it belongs to.
 */
export function LeadProposals() {
  const row = () => selected();
  const led = createMemo(() => {
    const now = row();
    return now && now.state !== "archived" ? ledTeamOf(teams.list(now.computer), now) : undefined;
  });
  const team = createMemo(() => {
    const now = row(), summary = led();
    if (!now || !summary || subagentOpen()) return undefined;
    const view = teams.team(now.computer, summary.id);
    // The live read has the last word on who leads: a roster change keeps no cards.
    return view && leadsTeam(view, now) ? view : undefined;
  });
  // A sub-team's lead session is also a member of the parent team, which the
  // sessions page may already follow: follow the sub-team's read while its
  // cards show. Only a different team re-subscribes, not every list poll.
  createEffect(on(() => {
    const now = row(), summary = led();
    return now && summary && !subagentOpen() ? `${now.computer}\n${summary.id}` : "";
  }, (key) => {
    if (!key) return;
    const at = key.indexOf("\n");
    const hide = showTeam(key.slice(0, at), key.slice(at + 1));
    onCleanup(hide);
  }));
  return (
    <Show when={team()}>{(view) => (
      <div class="cmp-col lead-proposals"><TeamProposals computer={row()!.computer} team={view()} /></div>
    )}</Show>
  );
}
