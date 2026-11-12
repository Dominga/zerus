import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount, type JSX } from "solid-js";
import { Portal } from "solid-js/web";
import { tr } from "../../i18n";
import { AgentGlyph, Icon } from "../lib/icons";
import { agentKind } from "../../store/catalog";
import { memberRow } from "../../store/teams";
import { accountsPage, loadAccounts } from "../../store/accounts";
import { accountChoices } from "../../store/newTeamSources";
import { createModelCatalogs, effortOptions, modelOptions } from "./catalogControls";
import { normalizeModelCatalog, selectModel } from "../provider-selection";
import { teamRoleModels } from "../../bridge";
import { composer } from "../../store/composer";
import { selected, setDetailTab, setSelectedKey } from "../../store/ui";
import { useModel, windowPort } from "./ProposalCards";
import { TeamActions } from "./actions";
import { QuestionAnswer, TextDialog } from "./TeamActions";
import { SwapMemberDialog } from "./SwapMemberDialog";
import { roleStyle } from "./roles";
import { memberProgress, memberStateColor, unreadBadge, type MemberStatus } from "./team-model";
import { answerRow, newestAnswers, permissionSummary } from "./permissionAnswers";
import { now } from "../../store/ui";
import { accountLabel, limitText, memberMenu, memberAccount, moveAffected, reportsOf, scaleRoom, waitingForTask, type MemberMenuId } from "./members";
import type { ComputerId, TeamMemberView, TeamPulseView, TeamView } from "../../types";
import { TextInput } from "../lib/TextField";

// Task 199 (owner, 2026-11-07: Tauri parity with Qt): a member's card in the
// side column as Qt's TeamWorkspace::members draws it, and its menu
// (TeamWorkspace::memberMenu) with the actions in the core.

/** Task 133: a pulse's lines as the member row shows them, at most three. */
export function pulseLines(pulse: TeamPulseView): string[] {
  return [
    pulse.done ? `${tr("Done")}: ${pulse.done}` : "",
    pulse.next ? `${tr("Next")}: ${pulse.next}` : "",
    pulse.blocked ? `${tr("Blocked")}: ${pulse.blocked}` : "",
  ].filter(Boolean);
}

function pulseText(pulse: TeamPulseView): string {
  const by = pulse.source === "note" ? tr("the member's own note") : tr("Zerus's summary of its last turn");
  return [...pulseLines(pulse), by].join("\n");
}

const AGENTS = ["claude", "codex", "kimi", "dsh", "opencode"];

/**
 * The Tauri client sends a session message only from a composer: the member
 * becomes the selected session, keeps the Team tab, and the text waits in its
 * composer for the owner's Send (Qt's Message... sends at once).
 */
export function toComposer(computer: ComputerId, member: Pick<TeamMemberView, "session"> | undefined, text: string) {
  const row = memberRow(computer, member);
  if (!row) return false;
  if (selected()?.key !== row.key) { setSelectedKey(row.key); setDetailTab("team"); }
  let tries = 0;
  const put = () => {
    const view = composer.view();
    if (selected()?.key === row.key && view && !composer.locked()) {
      const draft = composer.text();
      const next = draft.trim() ? `${draft}\n${text}` : text;
      composer.input(next, next.length, next.length);
      return;
    }
    if (++tries < 40) setTimeout(put, 50);
  };
  put();
  return true;
}

function clock(seconds: number): string {
  if (!seconds) return "";
  const at = new Date(seconds * 1000);
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
}

export function MemberCard(props: {
  computer: ComputerId; team: TeamView; member: TeamMemberView; status: MemberStatus; depth: number;
  focused: boolean; onFocus: () => void; folded: boolean; onFold: () => void; onTask: (id: number) => void;
}): JSX.Element {
  const { model, tick } = useModel((changed) => new TeamActions(windowPort, () => ({ computer: props.computer, team: props.team }), changed));
  const read = () => (tick(), model);
  const own = createMemo(() => memberProgress(props.team, props.member));
  const reports = () => reportsOf(props.team, props.member.id).length;
  const [tasksOpen, setTasksOpen] = createSignal(false);
  // Task #347: the card shows a header and one status line; the account, pulse,
  // marks, tasks and unread wait behind this fold (a request stays in full).
  const [detailsOpen, setDetailsOpen] = createSignal(false);
  // Task 315 (Qt U17): the answered permissions fold to one line and open to the newest five.
  const [answersOpen, setAnswersOpen] = createSignal(false);
  const answers = () => props.member.permission_answers ?? [];
  const [menu, setMenu] = createSignal<{ left: number; top: number; assign?: boolean } | undefined>();
  const [dialog, setDialog] = createSignal<MemberMenuId | "add" | undefined>();
  const [denyReason, setDenyReason] = createSignal("");
  const role = () => props.member.role || props.member.id;
  const permission = () => props.member.permission ?? null;
  const answerable = () => {
    const question = permission();
    const native = question?.answer && typeof question.answer === "object" ? question.answer : undefined;
    return !!question?.question_id && !!(question.question_hash || question.expected_question_hash) && !!native?.question_id && !!native.allow && !!native.deny;
  };
  const open = (left: number, top: number, assign = false) => setMenu({ left: Math.max(8, Math.min(left, window.innerWidth - 300)), top, assign });
  createEffect(() => {
    if (!menu()) return;
    const close = (event: Event) => {
      if (event instanceof KeyboardEvent && event.key !== "Escape") return;
      if (event.target instanceof Element && event.target.closest(".team-member-menu, [data-action='member-more'], [data-action='member-assign']")) return;
      setMenu(undefined);
    };
    document.addEventListener("pointerdown", close, true);
    document.addEventListener("keydown", close, true);
    onCleanup(() => { document.removeEventListener("pointerdown", close, true); document.removeEventListener("keydown", close, true); });
  });
  const choose = (id: MemberMenuId, choice?: string) => {
    setMenu(undefined);
    const member = props.member.id;
    switch (id) {
      case "open": { const row = memberRow(props.computer, props.member); if (row) { setSelectedKey(row.key); setDetailTab("activity"); } return; }
      case "pause": return void model.pauseMember(member);
      case "resume": return void model.resumeMember(member);
      case "relaunch": return void model.relaunch(member);
      case "drain": return void model.drain(member, role());
      case "undrain": return void model.drain(member, role(), true);
      case "remove": return void model.removeMember(member, role());
      case "sub_lead": return void model.subLead(member, !props.member.sub);
      case "reports_to": if (choice) void model.reportsTo(member, choice); return;
      default: return setDialog(id);
    }
  };
  const assignable = () => props.team.tasks.filter((task) => !["accepted", "rejected", "cancelled", "done"].includes(task.status) && task.assignee !== props.member.id);
  // Task 355 (owner, 2026-11-08): the row shows its state as a word and its
  // task number beside it, so a queued task is not read as work; the row of
  // the session being looked at says "you".
  const taskNumber = () => props.status.task || props.member.queue[0] || 0;
  const taskTitle = () => props.team.tasks.find((task) => task.id === taskNumber())?.title ?? "";
  const detail = () => {
    if (props.member.error) return props.member.error;
    if (taskTitle() && ["working", "sent", "submitted", "in_review", "in_test"].includes(props.status.kind)) return taskTitle();
    return props.status.text === props.status.short ? "" : props.status.text;
  };
  const isYou = () => {
    const row = memberRow(props.computer, props.member);
    return !!row && row.key === selected()?.key;
  };
  const stop = (event: Event) => event.stopPropagation();
  return (
    <div class="team-member team-member-card" data-member={props.member.id} data-kind={props.status.kind} data-state-color={memberStateColor(props.status.kind)} role="button" tabindex="0"
      classList={{ problem: props.status.problem, [`tone-${props.status.tone}`]: true, lead: props.member.lead, focused: props.focused, sub: !!props.member.sub, planned: props.status.kind === "not_created" }}
      style={{ "margin-left": `${Math.max(0, props.depth - 1) * 14}px` }}
      title={props.focused ? tr("Show every member") : tr("Show only %1's tasks", role())}
      onClick={() => props.onFocus()} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); props.onFocus(); } }}>
      <span class="team-tile" title={props.member.agent}><AgentGlyph agent={agentKind(props.member.agent)} size={14} /></span>
      <div class="team-member-main">
        <div class="team-member-line">
          <span class="role-badge ell" style={roleStyle(props.member.role, props.member.lead)}>{role()}</span>
          <Show when={isYou()}><span class="team-you" data-you>{tr("you")}</span></Show>
          <Show when={props.member.sub}>
            <button type="button" class="team-sublead" data-action="member-fold" aria-expanded={!props.folded} title={tr("Show or hide its reports")}
              onClick={(event) => { stop(event); props.onFold(); }}>
              <Icon name={props.folded ? "chevron-right" : "chevron-down"} size={10} />
              {reports() === 1 ? tr("sub-lead, 1 report") : tr("sub-lead, %1 reports", reports())}
            </button>
          </Show>
          <Show when={props.member.question}><span class="team-q" title={tr("A question waits in its session")}>?1</span></Show>
          <span class="dim ell team-model">{props.member.model || props.member.agent}</span>
          <span class="grow" />
          <span class="mono dim team-member-progress" title={tr("%1 of %2 tasks accepted", own().accepted, own().total)}>{own().accepted}/{own().total}</span>
          <button type="button" class="icon-btn team-member-details" data-action="member-details" aria-expanded={detailsOpen()}
            aria-label={tr("Member details")} title={tr("Member details")}
            onClick={(event) => { stop(event); setDetailsOpen(!detailsOpen()); }}>
            <Icon name={detailsOpen() ? "chevron-down" : "chevron-right"} size={12} />
          </button>
          <button type="button" class="icon-btn team-member-more" data-action="member-more" aria-label={tr("Member actions")} title={tr("Member actions")}
            aria-haspopup="menu" disabled={read().busy}
            onClick={(event) => { stop(event); const box = event.currentTarget.getBoundingClientRect(); if (menu()) setMenu(undefined); else open(box.left - 220, box.bottom + 4); }}>
            <Icon name="ellipsis" size={14} />
          </button>
        </div>
        <Show when={detailsOpen()}>
          <Show when={props.focused}><div class="team-member-focus">{tr("Only its tasks are shown, click the card for all")}</div></Show>
          <Show when={props.member.pulse}>{(pulse) => (
            <div class="team-member-pulse" classList={{ blocked: !!pulse().blocked }} title={pulseText(pulse())}>
              <For each={pulseLines(pulse())}>{(line) => <span class="ell">{line}</span>}</For>
            </div>
          )}</Show>
          <div class="team-member-line sub">
            <Show when={unreadBadge(props.member)}>{(badge) => (
              <span class="team-unread mono" classList={{ urgent: badge().urgent }} title={badge().tip}>{badge().text}</span>
            )}</Show>
            <Show when={props.team.status !== "draft"}>
              <span class="dim ell team-account" data-account={memberAccount(props.member)}
                title={props.member.session && props.member.session_account != null ? tr("Its session runs on %1", accountLabel(props.member, {}, accountsPage())) : tr("Account: %1", accountLabel(props.member, {}, accountsPage()))}>
                {accountLabel(props.member, {}, accountsPage())}
              </span>
            </Show>
          </div>
          <Show when={waitingForTask(props.team, props.member)}>
            <div class="team-member-line sub" onClick={stop}>
              <button type="button" class="btn ghost small" data-action="member-assign"
                onClick={(event) => { const box = event.currentTarget.getBoundingClientRect(); open(box.left, box.bottom + 4, true); }}>{tr("Assign task…")}</button>
              <button type="button" class="btn ghost small" data-action="member-ask-lead"
                onClick={() => toComposer(props.computer, props.team.members.find((found) => found.lead), tr("I added %1 to the team. Plan a task for it and reassign it with hgs team reassign.", role()))}>
                {tr("Ask lead to plan for it")}</button>
            </div>
          </Show>
        </Show>
        <Show when={waitingForTask(props.team, props.member)} fallback={
          <div class="team-member-line sub">
            <span class={`team-status tone-${props.status.tone}`} title={props.status.text} data-state={props.status.kind}><Icon name={props.status.icon} size={12} />{props.status.short}</span>
            <Show when={taskNumber() > 0}><span class="mono dim team-member-task" data-task-number={taskNumber()}>#{taskNumber()}</span></Show>
            <Show when={detail()}><span class="ell team-member-text" title={props.member.error || props.status.text}>{detail()}</span></Show>
          </div>
        }>
          <div class="team-member-line sub">
            <span class="team-status" title={tr("Waiting for a task")} data-state="waiting"><Icon name="hourglass" size={12} />{tr("Waiting")}</span>
            <span class="ell team-member-text">{tr("Waiting for a task")}</span>
          </div>
        </Show>
        <Show when={detailsOpen()}>
        <Show when={props.member.draining || props.member.limited || props.member.host_shared || (props.member.services?.length ?? 0) > 0 || props.member.budget}>
          <div class="team-member-marks">
            <Show when={props.member.draining}>
              <span class="team-mark warn" data-mark="draining" title={tr("Finishes its open work, gets nothing new, then its session is paused and it leaves the team. Cancel drain in its menu.")}>{tr("Draining")}</span>
            </Show>
            <Show when={props.member.limited}>
              <span class="team-mark warn" data-mark="limited" title={props.member.limited?.detail || undefined}>{limitText(props.member, clock)}</span>
            </Show>
            <Show when={props.member.host_shared}>
              <span class="team-mark" data-mark="shared-host" title={tr("Its DeepSeek host serves other members too.")}>{tr("Shared host")}</span>
            </Show>
            <Show when={(props.member.services?.length ?? 0) > 0}>
              <span class="team-mark" data-mark="services"
                title={[...(props.member.services ?? []).map((service) => tr("%1, since %2", service.name, clock(service.since) || tr("unknown"))), tr("These outlive the session and are not Background work.")].join("\n")}>
                {props.member.services!.length === 1 ? tr("1 service") : tr("%1 services", props.member.services!.length)}</span>
            </Show>
            <Show when={props.member.budget}>{(budget) => (
              <span class="team-mark" classList={{ warn: budget().state === "amber", danger: budget().state === "red" }} data-mark="budget"
                title={[budget().label, tr("%1 of %2 min, red at %3 min", budget().elapsed_minutes, budget().minutes, budget().red_minutes)].filter(Boolean).join("\n")}>
                {tr("Budget %1: %2 min", budget().state, budget().elapsed_minutes)}</span>
            )}</Show>
          </div>
        </Show>
        <Show when={props.member.queue.length > 0}>
          <button type="button" class="team-member-tasks" data-action="member-tasks" aria-expanded={tasksOpen()} onClick={(event) => { stop(event); setTasksOpen(!tasksOpen()); }}>
            <Icon name={tasksOpen() ? "chevron-down" : "chevron-right"} size={10} />
            {props.member.queue.length === 1 ? tr("1 task") : tr("%1 tasks", props.member.queue.length)}
          </button>
          <Show when={tasksOpen()}>
            <div class="team-member-queue" onClick={stop}>
              <For each={props.member.queue.slice(0, 5)}>{(id, index) => {
                const task = () => props.team.tasks.find((found) => found.id === id);
                return (
                  <button type="button" class="team-member-queue-row" classList={{ current: index() === 0 }} data-task={id} onClick={() => props.onTask(id)}>
                    <span class="mono dim">#{id}</span><span class="ell">{task()?.title ?? ""}</span>
                  </button>
                );
              }}</For>
              <Show when={props.member.queue.length > 5}><span class="dim">{tr("+%1 more", props.member.queue.length - 5)}</span></Show>
            </div>
          </Show>
        </Show>
        <Show when={answers().length > 0}>
          <button type="button" class="team-member-tasks team-member-answers" data-action="member-permissions" aria-expanded={answersOpen()}
            title={answersOpen() ? tr("%1 answers, click to fold them", answers().length) : tr("%1 answers, click to expand them", answers().length)}
            onClick={(event) => { stop(event); setAnswersOpen(!answersOpen()); }}>
            <Icon name={answersOpen() ? "chevron-down" : "chevron-right"} size={10} />
            <span class="ell">{permissionSummary(answers(), now())}</span>
          </button>
          <Show when={answersOpen()}>
            <div class="team-member-queue team-member-answer-list" onClick={stop}>
              <For each={newestAnswers(answers()).shown}>{(answer) => {
                const row = () => answerRow(answer, now());
                return (
                  <div class="team-member-answer" data-decision={answer.decision} title={row().tip}>
                    <span class="ell" classList={{ warn: row().denied, accent: !row().denied }}>{row().decision}</span>
                    <Show when={row().command}><span class="ell mono">{row().command}</span></Show>
                    <Show when={row().reason}><span class="ell dim">{row().reason}</span></Show>
                  </div>
                );
              }}</For>
              <Show when={newestAnswers(answers()).more > 0}><span class="dim">{tr("+%1 more", newestAnswers(answers()).more)}</span></Show>
            </div>
          </Show>
        </Show>
        </Show>
        <Show when={permission() && (permission()!.command || permission()!.tool || permission()!.subagent_confirmation)}>
          <div class="team-member-permission" data-member={props.member.id} onClick={stop}>
            <div class="team-member-line">
              <Show when={permission()!.tool}><span class="team-chip warn">{permission()!.tool}</span></Show>
              <span class="dim ell">{permission()!.subagent_confirmation ? tr("Start subagents") : permission()!.command ? tr("command from the tool input") : tr("the agent did not show the command")}</span>
            </div>
            <Show when={permission()!.command}><code class="team-permission-command">{permission()!.command}</code></Show>
            <Show when={permission()!.reason}><div class="dim">{tr("Agent: %1", permission()!.reason!)}</div></Show>
            <div class="team-member-line">
              <TextInput class="field grow" placeholder={tr("Why deny? The member receives this")} value={denyReason()} onInput={(event) => setDenyReason(event.currentTarget.value)}
                disabled={!answerable()} data-field="deny-reason" />
              <button type="button" class="btn small danger" data-action="permission-deny" disabled={!answerable() || read().busy}
                title={answerable() ? undefined : tr("This hgs sends no answer ids for the question; answer it in the session.")}
                onClick={() => void model.answerMember(props.member.id, permission()!, false, denyReason())}>{tr("Deny")}</button>
              <button type="button" class="btn small primary" data-action="permission-allow" disabled={!answerable() || read().busy}
                title={answerable() ? undefined : tr("This hgs sends no answer ids for the question; answer it in the session.")}
                onClick={() => void model.answerMember(props.member.id, permission()!, true)}>{permission()!.subagent_confirmation ? tr("Allow all") : tr("Allow")}</button>
            </div>
          </div>
        </Show>
        {/* Owner, 2026-11-09: a member's question shows in one place only, its card. */}
        <For each={(props.team.tasks ?? []).filter((task) => task.assignee === props.member.id).flatMap((task) => (task.questions ?? []).map((question) => ({ task, question })))}>{(entry) => (
          <div class="team-member-permission team-member-question" data-member={props.member.id} onClick={stop}>
            <div class="team-member-line"><span class="team-chip warn">{tr("Question")}</span><span class="dim ell">#{entry.task.id} {entry.task.title}</span></div>
            <QuestionAnswer computer={props.computer} team={props.team as never} task={entry.task} question={entry.question} />
          </div>
        )}</For>
        <Show when={read().notice}><div class="danger team-member-notice" role="status">{read().notice}</div></Show>
      </div>
      <Show when={menu()}>{(at) => (
        <Portal>
          <div class="menu team-member-menu" role="menu" aria-label={at().assign ? tr("Assign task") : tr("Member actions")} data-member={props.member.id}
            style={{ left: `${at().left}px`, top: `${at().top}px` }}>
            <Show when={at().assign} fallback={
              <For each={memberMenu(props.team, props.member)}>{(item) => (
                <>
                  <button type="button" role="menuitem" class="menu-item" data-item={item.id} disabled={!item.enabled || (!!item.choices && item.choices.length === 0)}
                    aria-disabled={!item.enabled} title={item.tip} onClick={() => { if (!item.choices) choose(item.id); }}>{item.text}</button>
                  <Show when={item.choices && item.enabled}>
                    <For each={item.choices}>{(choice) => (
                      <button type="button" role="menuitemradio" aria-checked={choice.checked} class="menu-item team-menu-sub" data-parent={choice.id}
                        onClick={() => choose("reports_to", choice.id)}>
                        <span class="team-phase-check">{choice.checked ? <Icon name="check" size={12} /> : null}</span>{choice.text}
                      </button>
                    )}</For>
                  </Show>
                </>
              )}</For>
            }>
              <For each={assignable()} fallback={<button type="button" class="menu-item" disabled aria-disabled>{tr("No open task")}</button>}>{(task) => (
                <button type="button" role="menuitem" class="menu-item" data-task={task.id}
                  onClick={() => { setMenu(undefined); void model.reassign(task, props.member.id); }}>
                  {`#${task.id} ${task.title}`}{props.team.ready.includes(task.id) ? tr(", ready") : ""}
                </button>
              )}</For>
            </Show>
          </div>
        </Portal>
      )}</Show>
      <Show when={dialog() === "message"}>
        <TextDialog title={tr("Message %1", role())} hint={tr("The text waits in its session's composer; Send there delivers it.")} placeholder={tr("Message")}
          action={tr("Put in its composer")} close={() => setDialog(undefined)} submit={(text) => void toComposer(props.computer, props.member, text.trim())} />
      </Show>
      <Show when={dialog() === "iron_rules"}>
        <TextDialog title={tr("%1: member iron rules", role())} hint={tr("Mandatory additions to the member's rules, one per line.")} placeholder={tr("One rule per line")}
          action={tr("Save")} initial={(props.member.iron_rules ?? []).join("\n")} count={() => ({ count: "", ok: true })} close={() => setDialog(undefined)}
          submit={(text) => void model.ironRules(props.member.id, text.split("\n").map((line) => line.trim()).filter(Boolean))} />
      </Show>
      <Show when={dialog() === "scale"}>
        <ScaleDialog role={role()} member={props.member} room={scaleRoom(props.team)} close={() => setDialog(undefined)}
          submit={(count) => void model.scale({ role: props.member.role, agent: props.member.agent, count, model: props.member.model || undefined, effort: props.member.effort || undefined, account: props.member.account || undefined })} />
      </Show>
      <Show when={dialog() === "swap"}>
        {/* Task 218: the Swap dialog offers the agent's real models, efforts and accounts, or adopts a live session. */}
        <SwapMemberDialog computer={props.computer} team={props.team} member={props.member} close={() => setDialog(undefined)} />
      </Show>
      <Show when={dialog() === "move_account"}>
        <MoveAccountDialog computer={props.computer} team={props.team} member={props.member} close={() => setDialog(undefined)}
          submit={(to) => void model.moveAccount(props.member.agent, memberAccount(props.member), to)} />
      </Show>
    </div>
  );
}

function useModal(close: () => void) {
  let dialog!: HTMLDialogElement;
  onMount(() => dialog.showModal());
  onCleanup(() => { if (dialog.open) dialog.close(); });
  return { ref: (el: HTMLDialogElement) => { dialog = el; }, cancel: (event: Event) => { event.preventDefault(); close(); } };
}

/** U49: Add more X... (Qt scaleRole): how many, up to the room the limit leaves. */
function ScaleDialog(props: { role: string; member: TeamMemberView; room: number; close: () => void; submit: (count: number) => void }) {
  const modal = useModal(props.close);
  const [count, setCount] = createSignal(1);
  const agent = [props.member.agent, props.member.model].filter(Boolean).join(" ");
  return (
    <dialog ref={modal.ref} class="dlg team-member-dialog" onCancel={modal.cancel} onClick={(event) => event.stopPropagation()}>
      <div class="dlg-head"><h2>{tr("Add more %1", props.role)}</h2></div>
      <div class="dlg-body">
        <p class="team-confirm-text">{tr("How many more %1 members, each on %2?", props.role, agent)}</p>
        <TextInput class="field" type="number" min={1} max={props.room} value={count()} data-field="scale-count"
          onInput={(event) => setCount(Math.max(1, Math.min(props.room, Number(event.currentTarget.value) || 1)))} />
      </div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn" onClick={props.close}>{tr("Cancel")}</button>
        <button type="button" class="btn primary" onClick={() => { props.submit(count()); props.close(); }}>{tr("Add")}</button>
      </div>
    </dialog>
  );
}

/** U54: Move to another account... (Qt accountMoveDialog): every member of this agent on its account moves, the lead included. */
function MoveAccountDialog(props: { computer: ComputerId; team: TeamView; member: TeamMemberView; close: () => void; submit: (to: string) => void }) {
  const modal = useModal(props.close);
  const from = memberAccount(props.member);
  const affected = moveAffected(props.team, props.member.agent, from).map((id) => props.team.members.find((found) => found.id === id)?.role || id);
  const [to, setTo] = createSignal("");
  const target = () => to().trim();
  const targets = () => accountChoices(accountsPage(), props.computer).filter((account) => account.provider === props.member.agent && account.id !== from);
  onMount(() => { void loadAccounts(); });
  return (
    <dialog ref={modal.ref} class="dlg team-member-dialog" onCancel={modal.cancel} onClick={(event) => event.stopPropagation()}>
      <div class="dlg-head"><h2>{tr("Move to another account")}</h2></div>
      <div class="dlg-body">
        <p class="team-confirm-text">{tr("Moves every %1 member on %2 to another account: %3.", props.member.agent, accountLabel(props.member, {}, accountsPage()), affected.join(", "))}</p>
        <label class="dim">{tr("To account")}</label>
        <select class="field" value={to()} onChange={(event) => setTo(event.currentTarget.value)} data-field="move-to">
          <option value="">{tr("Account id")}</option>
          <For each={targets()}>{(account) => <option value={account.id}>{account.signedIn ? account.label : tr("%1 (signed out)", account.label)}</option>}</For>
        </select>
      </div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn" onClick={props.close}>{tr("Cancel")}</button>
        <button type="button" class="btn primary" disabled={!target() || target() === from} onClick={() => { props.submit(target()); props.close(); }}>{tr("Move")}</button>
      </div>
    </dialog>
  );
}

/** "+ Member" (Qt addMember): its role, agent, model, effort and account; it starts with the first task it gets. */
export function AddMemberDialog(props: { computer: ComputerId; close: () => void; submit: (member: Record<string, unknown>) => void }) {
  const modal = useModal(props.close);
  const [role, setRole] = createSignal("");
  const [agent, setAgent] = createSignal("claude");
  const [model, setModel] = createSignal("");
  const [effort, setEffort] = createSignal("");
  const [account, setAccount] = createSignal("");
  const [modelTick, setModelTick] = createSignal(0);
  const catalogs = createModelCatalogs((computer, agentId, accountId) => teamRoleModels(computer, agentId, accountId), () => setModelTick((n) => n + 1));
  const selection = () => ({ computer: props.computer, agent: agent(), account: account(), model: model(), effort: effort() });
  const accounts = () => accountChoices(accountsPage(), props.computer).filter((choice) => choice.provider === agent());
  const models = () => (modelTick(), modelOptions(selection(), catalogs.get(selection())));
  const efforts = () => (modelTick(), effortOptions(model(), catalogs.get(selection()), effort()));
  createEffect(() => catalogs.request(selection()));
  onMount(() => { void loadAccounts(); });
  const changeAgent = (value: string) => { setAgent(value); setAccount(""); setModel(""); setEffort(""); };
  const changeAccount = (value: string) => { setAccount(value); setModel(""); setEffort(""); };
  const changeModel = (value: string) => {
    const next = selectModel(normalizeModelCatalog(catalogs.get(selection()) ?? []), value, effort());
    setModel(next.modelId);
    setEffort(next.effortId);
  };
  const add = () => {
    const member: Record<string, unknown> = { role: role().trim(), agent: agent() };
    if (model().trim()) member.model = model().trim();
    if (effort()) member.effort = effort();
    if (account().trim()) member.account = account().trim();
    props.submit(member);
    props.close();
  };
  return (
    <dialog ref={modal.ref} class="dlg team-member-dialog" onCancel={modal.cancel}>
      <div class="dlg-head"><h2>{tr("Add a member")}</h2></div>
      <div class="dlg-body">
        <p class="team-confirm-text">{tr("Add a member; it starts with the first task it gets")}</p>
        <label class="dim">{tr("Role")}</label>
        <TextInput class="field" value={role()} onInput={(event) => setRole(event.currentTarget.value)} data-field="member-role" />
        <label class="dim">{tr("Agent")}</label>
        <select class="field" value={agent()} onChange={(event) => changeAgent(event.currentTarget.value)} data-field="member-agent">
          <For each={AGENTS}>{(name) => <option value={name}>{name}</option>}</For>
        </select>
        <label class="dim">{tr("Model")}</label>
        <TextInput class="field" list="team-add-member-models" value={model()} onInput={(event) => changeModel(event.currentTarget.value)} />
        <datalist id="team-add-member-models"><For each={models()}>{(option) => <option value={option.id}>{option.label}</option>}</For></datalist>
        <label class="dim">{tr("Effort")}</label>
        <select class="field" value={effort()} onChange={(event) => setEffort(event.currentTarget.value)}>
          <For each={efforts()}>{(level) => <option value={level}>{level || tr("its effort")}</option>}</For>
        </select>
        <label class="dim">{tr("Account")}</label>
        <select class="field" value={account()} onChange={(event) => changeAccount(event.currentTarget.value)}>
          <option value="">{tr("Default account")}</option>
          <For each={accounts()}>{(choice) => <option value={choice.id}>{choice.signedIn ? choice.label : tr("%1 (signed out)", choice.label)}</option>}</For>
        </select>
      </div>
      <div class="dlg-foot"><span class="spacer" />
        <button type="button" class="btn" onClick={props.close}>{tr("Cancel")}</button>
        <button type="button" class="btn primary" disabled={!role().trim()} onClick={add}>{tr("Add")}</button>
      </div>
    </dialog>
  );
}

