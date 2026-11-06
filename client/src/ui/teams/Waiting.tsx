import { For, Match, Show, Switch, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { toast } from "../../store/notices";
import { answeredWaiting, memberRow, showWaiting, teams, waiting } from "../../store/teams";
import { teamsWaitingOlder } from "../../bridge";
import { waitingNotices } from "../../store/waitingNotices";
import { setDetailTab, setPage, setSelectedKey } from "../../store/ui";
import { ProposalCardList, useModel, windowPort } from "./ProposalCards";
import {
  BANNER_ROWS, WaitingAnswer, acceptanceSummary, approves, opensCard, toolOffer, waitingActionLabel, waitingAge,
  waitingCount, waitingFields, waitingKindIcon, waitingKindLabel, type QuestionChoice,
} from "./decisions";
import type { TeamWaitingItem } from "../../types";

// Waiting for you (task 11, Qt TeamWaitingPage, TeamWaitingBanner and
// TeamWaitingAnswer): what waits for the owner across this computer's teams,
// newest first, each with its kind, team and age. A one-click decision is
// answered in its row (Approve… with the team's question, Answer for a
// proposal, an addition, a permission or question, a phase acceptance and a
// missing tool); a report and a held task open their team. An answered item
// leaves at once; one that changed meanwhile says so and sends nothing. No
// dots separate the parts of a row.

/** Opens the team of an item at its coordinator's session, on the Team tab. */
export function openTeam(item: TeamWaitingItem) {
  if (!item.team) return;
  const summary = teams.list("")?.teams.find((team) => team.id === item.team);
  const lead = summary?.members.find((member) => member.lead);
  const row = memberRow("", lead);
  if (!row) { toast(tr("Open team"), tr("The coordinator's session is not on the session list.")); return; }
  setPage("sessions");
  setSelectedKey(row.key);
  setDetailTab("team");
}

/** Notifies each new item once while Waiting for you is shown. */
function announce(items: readonly TeamWaitingItem[], complete: boolean) {
  for (const notice of waitingNotices().observe(items, complete)) toast(notice.title, notice.body, "info");
}

/** The answer of one item, in its row. */
function Answer(props: { item: TeamWaitingItem }) {
  const { model, tick } = useModel((changed) => new WaitingAnswer(props.item, windowPort, "", changed, answeredWaiting));
  const read = () => (tick(), model);
  onMount(() => void model.open());
  const fields = waitingFields(props.item);
  const [typed, setTyped] = createSignal("");
  const [comment, setComment] = createSignal("");
  const [reason, setReason] = createSignal("");
  const [choices, setChoices] = createSignal(new Map<string, QuestionChoice>());
  const choose = (id: string, update: (choice: QuestionChoice) => QuestionChoice) => {
    const next = new Map(choices());
    next.set(id, update(next.get(id) ?? { chosen: new Set(), other: false, text: "" }));
    setChoices(next);
  };
  const off = () => read().busy || read().done;
  const offer = toolOffer(props.item);
  return (
    <div class="team-waiting-answer" onClick={(event) => event.stopPropagation()}>
      <Show when={read().notice}><div class="team-waiting-notice" role="status">{read().notice}</div></Show>
      <Show when={read().reading && !read().team && props.item.action !== "decide_tool"}><div class="dim">{tr("Reading the team…")}</div></Show>
      <Switch>
        <Match when={props.item.action === "decide_proposal" && read().proposals}>
          <ProposalCardList cards={read().proposals!} tick={tick} />
        </Match>
        <Match when={props.item.action === "confirm_addition" && read().addition()}>{(addition) => (
          <div class="team-waiting-card">
            <For each={addition().tasks ?? []}>{(task) => <div class="team-waiting-line">{task.title}</div>}</For>
            <div class="team-proposal-answers">
              <input class="field" placeholder={tr("Type add to confirm")} value={typed()} disabled={off()} onInput={(event) => setTyped(event.currentTarget.value)} />
              <button class="btn small" disabled={off()} onClick={() => void read().answerAddition(false)}>{tr("Cancel")}</button>
              <button class="btn small primary" disabled={off() || typed().trim() !== "add"} onClick={() => void read().answerAddition(true, typed())}>{tr("Add")}</button>
            </div>
          </div>
        )}</Match>
        <Match when={props.item.action === "answer_permission" && read().question()}>{(question) => (
          <Show when={(question().questions?.length ?? 0) > 0} fallback={
            <div class="team-waiting-card">
              <div class="team-waiting-line"><b>{question().tool}</b></div>
              <code class="team-waiting-command">{question().command || question().reason}</code>
              <div class="team-proposal-answers">
                <input class="field" maxlength={2000} placeholder={tr("Why deny? The member receives this")} value={reason()} disabled={off()} onInput={(event) => setReason(event.currentTarget.value)} />
                <button class="btn small danger" disabled={off()} onClick={() => void read().answerPermission(false, reason())}>{tr("Deny")}</button>
                <button class="btn small primary" disabled={off()} onClick={() => void read().answerPermission(true)}>{tr("Allow")}</button>
              </div>
            </div>
          }>
            <div class="team-waiting-card">
              <For each={question().questions ?? []}>{(asked) => (
                <div class="team-waiting-question" role="group" aria-label={asked.question}>
                  <div class="team-waiting-line"><b>{asked.question}</b></div>
                  <For each={asked.options ?? []}>{(option) => (
                    <label class="team-proposal-option">
                      <input type={asked.multi_select ? "checkbox" : "radio"} name={`q-${props.item.id}-${asked.id}`} disabled={off()}
                        checked={choices().get(asked.id)?.chosen.has(option.id) ?? false}
                        onChange={(event) => choose(asked.id, (choice) => {
                          const chosen = asked.multi_select ? new Set(choice.chosen) : new Set<string>();
                          if (event.currentTarget.checked) chosen.add(option.id); else chosen.delete(option.id);
                          return { ...choice, chosen, other: asked.multi_select ? choice.other : false };
                        })} />
                      <span title={option.description}>{option.label}</span>
                    </label>
                  )}</For>
                  <Show when={asked.allow_other !== false}>
                    <label class="team-proposal-option">
                      <input type={asked.multi_select ? "checkbox" : "radio"} name={`q-${props.item.id}-${asked.id}`} disabled={off()} checked={choices().get(asked.id)?.other ?? false}
                        onChange={(event) => choose(asked.id, (choice) => ({ ...choice, other: event.currentTarget.checked, chosen: asked.multi_select ? choice.chosen : new Set() }))} />
                      {tr("Other")}
                    </label>
                    <Show when={choices().get(asked.id)?.other}>
                      <input class="field" maxlength={4000} placeholder={tr("Your answer")} disabled={off()} value={choices().get(asked.id)?.text ?? ""}
                        onInput={(event) => { const text = event.currentTarget.value; choose(asked.id, (choice) => ({ ...choice, text })); }} />
                    </Show>
                  </Show>
                </div>
              )}</For>
              <div class="team-proposal-answers"><span class="grow" />
                <button class="btn small primary" disabled={off() || !questionReady(question().questions ?? [], choices())} onClick={() => void read().answerQuestion(choices())}>{tr("Answer")}</button>
              </div>
            </div>
          </Show>
        )}</Match>
        <Match when={props.item.action === "accept_phase" && read().team && !read().notice}>
          <div class="team-waiting-card">
            <Show when={acceptanceSummary(fields.summary)}><div class="team-waiting-line">{acceptanceSummary(fields.summary)}</div></Show>
            <div class="team-proposal-answers">
              <input class="field" maxlength={4000} placeholder={tr("What should the lead change?")} value={comment()} disabled={off()} onInput={(event) => setComment(event.currentTarget.value)} />
              <button class="btn small" disabled={off() || !comment().trim()} onClick={() => void read().sendBack(comment())}>{tr("Send back")}</button>
              <button class="btn small primary" disabled={off()} onClick={() => void read().acceptPhase()}>{tr("Accept phase")}</button>
            </div>
          </div>
        </Match>
        <Match when={props.item.action === "decide_tool"}>
          <div class="team-waiting-card">
            <div class="team-waiting-line"><b>{offer.head}</b></div>
            <Show when={offer.why}><div class="team-waiting-line">{tr("Needed for %1", offer.why)}</div></Show>
            <Show when={offer.saves}><div class="team-waiting-line dim">{tr("It saves %1", offer.saves)}</div></Show>
            <code class="team-waiting-command">{offer.command}</code>
            <Show when={offer.output}><pre class="team-waiting-output">{offer.output}</pre></Show>
            <div class="team-proposal-answers">
              <input class="field" maxlength={500} placeholder={tr("Why deny? Zerus remembers it")} value={reason()} disabled={off()} onInput={(event) => setReason(event.currentTarget.value)} />
              <button class="btn small danger" disabled={off()} onClick={() => void read().decideTool(false, reason())}>{tr("Deny")}</button>
              <button class="btn small primary" disabled={off() || !offer.canAllow} onClick={() => void read().decideTool(true)}>{offer.allow}</button>
            </div>
          </div>
        </Match>
      </Switch>
    </div>
  );
}

function questionReady(questions: NonNullable<import("../../types").TeamMemberQuestion["questions"]>, choices: Map<string, QuestionChoice>) {
  return questions.length > 0 && questions.every((asked) => {
    const choice = choices.get(asked.id);
    return !!choice && (choice.other ? choice.text.trim() !== "" : choice.chosen.size > 0);
  });
}

/** Approve… asks the team's question in place (the core's confirmation). */
function Approve(props: { item: TeamWaitingItem; label?: string }) {
  const { model, tick } = useModel((changed) => new WaitingAnswer(props.item, windowPort, "", changed, answeredWaiting));
  const read = () => (tick(), model);
  return (
    <>
      <button class="btn small primary" disabled={read().busy || read().done} onClick={(event) => { event.stopPropagation(); void model.approve(); }}>{props.label ?? tr("Approve…")}</button>
      <Show when={read().notice}><span class="team-waiting-notice inline" role="status">{read().notice}</span></Show>
    </>
  );
}

/** One row: kind, title, team and age; Open team; the answer in place. */
function Row(props: { item: TeamWaitingItem; compact?: boolean }) {
  const [open, setOpen] = createSignal(false);
  const item = () => props.item;
  return (
    <div class="team-waiting-row" classList={{ compact: props.compact, open: open() }} data-item={item().id} data-kind={item().kind}
      onClick={() => { if (!open()) openTeam(item()); }}>
      <div class="team-waiting-head">
        <span class="team-waiting-icon"><Icon name={waitingKindIcon(item().kind)} size={14} /></span>
        <span class="team-waiting-kind">{waitingKindLabel(item())}</span>
        <span class="team-waiting-title ell" title={item().title}>{item().title}</span>
        <span class="grow" />
        <span class="team-waiting-team ell" title={item().team ? item().team_name : tr("Project %1", item().team_name)}>{item().team_name}</span>
        <span class="dim team-waiting-age">{waitingAge(item().since)}</span>
        <Show when={approves(item().action)}><Approve item={item()} /></Show>
        <Show when={opensCard(item().action)}>
          <button class="btn small" classList={{ primary: !open() }} onClick={(event) => { event.stopPropagation(); setOpen(!open()); }}>{open() ? tr("Close") : tr("Answer")}</button>
        </Show>
        <Show when={item().team}>
          <button class="btn small ghost" onClick={(event) => { event.stopPropagation(); openTeam(item()); }}>{tr("Open team")}</button>
        </Show>
      </div>
      <Show when={open()}><Answer item={item()} /></Show>
    </div>
  );
}

/** The rail page. */
export function WaitingPage() {
  onCleanup(showWaiting());
  const view = () => waiting();
  const failure = () => teams.failure("", "waiting") ?? teams.followError("");
  const [loading, setLoading] = createSignal(false);
  createEffect(() => { const items = view()?.items; if (items) announce(items, !view()?.more); });
  const older = async () => {
    setLoading(true);
    try {
      await teamsWaitingOlder("");
    } finally {
      setLoading(false);
    }
  };
  return (
    <section class="team-waiting-page" aria-label={tr("Waiting for you")}>
      <header class="team-waiting-header">
        <span class="team-waiting-tile"><Icon name="bell" size={16} /></span>
        <h1>{tr("Waiting for you")}</h1>
        <span class="dim">{view() ? waitingCount(view()!.items, view()!.total) : ""}</span>
      </header>
      <Show when={failure()}><p class="team-waiting-error" role="alert">{failure()}</p></Show>
      <div class="team-waiting-list">
        <Show when={view()} fallback={<p class="dim team-empty">{tr("Reading what waits for you…")}</p>}>
          <For each={view()!.items} fallback={<p class="dim team-empty">{tr("Nothing waits for you")}</p>}>{(item) => <Row item={item} />}</For>
          <Show when={view()!.more}>
            <button type="button" class="btn ghost small" disabled={loading()} onClick={() => void older()}>
              {tr("Show older")}
            </button>
          </Show>
        </Show>
      </div>
    </section>
  );
}

/** The Team tab's banner: one item is one line; several fold into a count that opens to the newest rows. */
export function WaitingBanner() {
  onCleanup(showWaiting());
  const [open, setOpen] = createSignal(false);
  const items = () => waiting()?.items ?? [];
  createEffect(() => announce(items(), !waiting()?.more));
  return (
    <Show when={items().length > 0}>
      <div class="team-waiting-banner" role="region" aria-label={tr("Waiting for you")}>
        <Show when={items().length > 1} fallback={<Row item={items()[0]} compact />}>
          <div class="team-waiting-banner-head">
            <Icon name="bell" size={14} />
            <b>{tr("%1 wait for you", items().length)}</b>
            <span class="dim ell">{items()[0].title}</span>
            <span class="grow" />
            <button class="btn small ghost" onClick={() => setOpen(!open())}>{open() ? tr("Hide") : tr("Show all")}</button>
          </div>
          <Show when={open()}>
            <For each={items().slice(0, BANNER_ROWS)}>{(item) => <Row item={item} compact />}</For>
            <button class="btn small ghost" onClick={() => setPage("waiting")}>{tr("Open the waiting list (%1)", waiting()?.total ?? items().length)}</button>
          </Show>
        </Show>
      </div>
    </Show>
  );
}
