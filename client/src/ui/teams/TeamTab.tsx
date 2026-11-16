import { For, Show, batch, createEffect, createMemo, createSignal, on, onCleanup, onMount } from "solid-js";
import { tr } from "../../i18n";
import { selected, setPage } from "../../store/ui";
import { machineCards } from "../../store/machines";
import { newRequestId, teams, type TeamCall, type TeamOutcome, type UnresolvedCall } from "../../store/teams";
import { rowIdentity, rowIsSession } from "./session-binding";
import { navigationTeamFor } from "./lead-navigation";
import { teamCall, type TaskButton } from "./actions";
import { createTeamEditors, TeamEditButtons, type EditAction } from "./TeamEditors";
import { MemberCard } from "./MemberCard";
import { TeamBar } from "./team-bar";
import { TeamSettingsDialog } from "./TeamSettingsDialog";
import { TaskActions } from "./TeamActions";
import { TeamConfirmDialog } from "./TeamConfirmDialog";
import { stageLine } from "./task-line";
import { TextInput } from "../lib/TextField";
import { composing } from "../lib/keys";
import { Icon } from "../lib/icons";
import type { TeamView } from "./types";
import { boardColumns, stageAgrees, type BoardColumn } from "./pipeline-view";
import { followTeamScope, type TeamScope } from "./team-tab-scope";
import { TaskDependencies } from "./TaskDependencies";
import { PlanView } from "./PlanView";
import { LanesView } from "./LanesView";
import { TeamTaskCard } from "./TeamTaskCard";
import { TeamAdvanceBase } from "./TeamAdvanceBase";
import { PhasePane } from "./PhasePane";
import { phaseFocus } from "../../store/team-phases";
import { TeamDrawerButtons, TeamDrawers } from "./TeamDrawers";
import type { TeamDrawer } from "../../store/teamDrawers";
import { openTeamNode, setOpenTeamNode } from "./openNode";
import "../styles/teams.css";
import "../styles/team-scroll.css";

// The Team workspace adapts the fork's team bar, members, paged Board, task
// detail and decisions. Graph and Layers share the core's projection. Feed,
// Inbox, Reports and Cards read bounded pages; Lead chat uses the bound lead's
// ordinary guarded composer. Team actions use the store's existing intents.

function columnLabel(column: BoardColumn): string {
  switch (column.key) {
    case "queue": return tr("Queue");
    case "work": return tr("In work");
    case "submitted": return tr("Submitted");
    case "review": return tr("In review");
    case "test": return tr("In test");
    case "land": return tr("Landing");
    case "accepted": return tr("Accepted");
    case "cancelled": return tr("Cancelled");
    default: return column.role || column.key;
  }
}

/** Board pages read to find the node Open task names before a notice says so. */
export const OPEN_NODE_PAGES = 4;

/** The workspace, from a view the store has already projected. */
export function TeamWorkspace(props: {
  view: TeamView;
  providerHome?: string;
  busy: boolean;
  failure: { code: string; message: string } | null;
  actionFailure?: string | null;
  onAction: EditAction;
  /** A queue page by its cursor; null is the first page. */
  onPage: (cursor: string | null) => void;
  onAddTask: () => void;
  onSettingsSave?: (call: TeamCall) => Promise<TeamOutcome>;
  settingsBlocked?: boolean;
}) {
  const [planMode, setPlanMode] = createSignal<"board" | "graph" | "lanes" | "layers">("board");
  const [settingsView, setSettingsView] = createSignal<TeamView>();
  const [selectedNode, setSelectedNode] = createSignal<number | null>(null);
  const [drawer, setDrawer] = createSignal<TeamDrawer | null>(null);
  const [advance, setAdvance] = createSignal(false);
  const [narrow, setNarrow] = createSignal(false);
  const [membersOpen, setMembersOpen] = createSignal(false);
  let workspace!: HTMLDivElement;
  let inspector!: HTMLElement;
  let returnFocus: HTMLElement | null = null;
  // Match the workspace container query, including a narrow split pane in a
  // wide window. This observes layout only; it never reads team data.
  onMount(() => {
    const measure = (width: number) => { if (width > 0) setNarrow(width <= 760); };
    measure(workspace.getBoundingClientRect().width);
    const observer = new ResizeObserver((entries) => measure(entries[0].contentRect.width));
    observer.observe(workspace);
    onCleanup(() => observer.disconnect());
  });
  const rememberFocus = () => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && workspace.contains(active) && !inspector.contains(active)) returnFocus = active;
  };
  const focusTarget = () => returnFocus?.isConnected ? returnFocus : (workspace.querySelector<HTMLElement>('.team-task-selected,.team-plan-card[aria-pressed="true"],[data-lane-task][aria-pressed="true"]') ?? workspace.querySelector<HTMLElement>('.team-board-head button[aria-pressed="true"]'));
  const restoreFocus = (target: HTMLElement | null) => {
    target?.focus({ preventScroll: true });
    returnFocus = null;
  };
  /** The stage Open task named, shown in its parent card's detail. */
  const [focusStage, setFocusStage] = createSignal<number | null>(null);
  const [openNotice, setOpenNotice] = createSignal("");
  const [asking, setAsking] = createSignal<TaskButton | null>(null);
  const [reason, setReason] = createSignal("");
  let lookups = 0;
  /** The page cursors this lookup has been on: a cursor seen again ends it. */
  const looked = new Set<string>();
  // Only another team resets the selection: a refreshed view of the same team
  // keeps the open card and a reason being typed.
  const teamId = createMemo(() => props.view.bar.id);
  createEffect(on(teamId, () => setDrawer(null), { defer: true }));
  createEffect(on(teamId, () => setAdvance(false), { defer: true }));
  createEffect(on(teamId, () => { setSettingsView(undefined); setMembersOpen(false); returnFocus = null; if (planMode() === "layers" || planMode() === "lanes") setPlanMode("board"); setSelectedNode(null); setFocusStage(null); setOpenNotice(""); setAsking(null); setReason(""); lookups = 0; looked.clear(); }, { defer: true }));
  createEffect(() => { if (phaseFocus()?.team === teamId()) { setSelectedNode(null); setPlanMode("layers"); } });
  const groups = createMemo(() => boardColumns(props.view));
  const all = createMemo(() => props.view.groups.flatMap((group) => group.cards));
  // Waiting for you's Open task: the node it names, or the card a stage of that
  // number is folded into, is selected once. A node not on this page is looked
  // for from the first page through at most OPEN_NODE_PAGES pages; then a
  // notice says it was not found. The link is never dropped silently.
  createEffect(() => {
    const wanted = openTeamNode();
    if (!wanted || wanted.team !== props.view.bar.id) return;
    const cards = all();
    const own = cards.find((candidate) => candidate.node === wanted.node);
    const parent = own ? undefined : cards.find((candidate) => candidate.stages.some((stage) => stage.node === wanted.node));
    if (own || parent) {
      setSelectedNode((own ?? parent)!.node); setFocusStage(parent ? wanted.node : null);
      setOpenNotice(""); setOpenTeamNode(null); lookups = 0; looked.clear();
      return;
    }
    if (lookups > 0 && props.view.after) looked.add(props.view.after);
    const next = props.view.next && !looked.has(props.view.next) ? props.view.next : null;
    const cursor = lookups === 0 && props.view.after ? null : lookups < OPEN_NODE_PAGES ? next : null;
    if (cursor !== null || (lookups === 0 && props.view.after)) {
      lookups++;
      setOpenNotice(tr("Looking for task #%1 on the board's pages…", wanted.node));
      props.onPage(cursor);
      return;
    }
    setOpenNotice(tr("Task #%1 is not on the board's pages that were read.", wanted.node));
    setOpenTeamNode(null); lookups = 0; looked.clear();
  });
  const select = (node: number) => { rememberFocus(); batch(() => { setMembersOpen(false); setSelectedNode(node); setFocusStage(null); setOpenNotice(""); }); };
  // Dismissal is viewer state only. Drop this team's outstanding Open task
  // lookup too, so its late page response cannot select a task again.
  const clearPreview = () => {
    setSelectedNode(null);
    setFocusStage(null);
    setOpenNotice("");
    setAsking(null);
    setReason("");
    lookups = 0;
    looked.clear();
    if (openTeamNode()?.team === props.view.bar.id) setOpenTeamNode(null);
  };
  const dismissPreview = () => {
    const target = focusTarget();
    batch(() => { clearPreview(); setMembersOpen(false); });
    restoreFocus(target);
  };
  const showMembers = () => {
    rememberFocus();
    batch(() => { clearPreview(); setMembersOpen(true); });
  };
  const dismissKey = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented || composing(event)) return;
    const target = event.target;
    if (!(target instanceof Element) || target.closest('input,textarea,select,[contenteditable]:not([contenteditable="false"]),[role="textbox"]')) return;
    const overlay = [...document.querySelectorAll('dialog[open],[aria-modal="true"],[role="dialog"],[role="menu"]')]
      .some((element) => element !== inspector && !element.closest('dialog:not([open]),[hidden],[aria-hidden="true"]'));
    if (overlay) return;
    // This native listener runs before the graph's delegated Escape handler.
    // Let that handler cancel its current link/edge gesture on the first key.
    const graph = target.closest("[data-team-graph]");
    if (graph?.querySelector('.team-plan-edge.selected,[role="status"] button')) return;
    if (selectedNode() === null && !openNotice() && !membersOpen()) return;
    event.preventDefault();
    event.stopPropagation();
    dismissPreview();
  };
  const dismissBlank = (event: MouseEvent) => {
    if (event.defaultPrevented || event.button !== 0 || !(event.target instanceof Element)) return;
    if (event.target.closest('.team-task-card,button,a,input,textarea,select,[contenteditable],[role="button"]')) return;
    dismissPreview();
  };
  const card = createMemo(() => all().find((candidate) => candidate.node === selectedNode()) ?? null);
  const inspectorOpen = () => !!card() || membersOpen();
  createEffect(on(() => [narrow(), card()?.node, membersOpen()] as const, () => {
    if (!narrow() || !inspectorOpen()) return;
    rememberFocus();
    queueMicrotask(() => {
      if (!narrow() || !inspectorOpen()) return;
      const active = document.activeElement;
      // Resizing an open inspector must keep its unsent input and focus.
      if (active instanceof HTMLElement && inspector.contains(active) && !active.closest("[hidden]")) return;
      inspector.querySelector<HTMLElement>('[data-team-detail] button[aria-label], [data-team-members]:not([hidden]) button[data-team-members-close]')?.focus({ preventScroll: true });
    });
  }));
  const editors = createTeamEditors(() => props.view, props.onAction);
  const send = (kind: string, fields: Record<string, unknown> = {}) => { if (kind === "swap_member") { editors.open(kind, String(fields.member)); return; } props.onAction(kind, fields); setAsking(null); setReason(""); };
  return (
    <div ref={workspace} class="team-tab" data-team-tab on:keydown={dismissKey}>
      <TeamBar view={props.view} busy={props.busy} onAction={(kind) => kind === "advance_base" ? setAdvance(true) : send(kind)}
        onSettings={props.onSettingsSave ? () => setSettingsView(props.view) : undefined} settingsBlocked={props.settingsBlocked}
        drawers={<TeamDrawerButtons value={drawer()} onOpen={(kind) => setDrawer(drawer() === kind ? null : kind)} />} />
      <Show when={settingsView()}><TeamSettingsDialog view={props.view} providerHome={props.providerHome} busy={props.busy || !!props.settingsBlocked}
        onSave={(call) => props.onSettingsSave!(call)} onClose={() => setSettingsView(undefined)} /></Show>
      <Show when={advance()}><TeamAdvanceBase team={props.view.bar.id} busy={props.busy}
        onCancel={() => setAdvance(false)} onAdvance={(commit, revision) => { props.onAction("advance_base", { commit }, revision); setAdvance(false); }} /></Show>
      <Show when={drawer()} keyed>{(kind) => <TeamDrawers kind={kind} view={props.view} busy={props.busy} onAction={props.onAction} onClose={() => setDrawer(null)} />}</Show>
      <TeamEditButtons view={props.view} busy={props.busy} open={editors.open} />
      {editors.dialogs()}
      <Show when={openNotice()}>{(text) => <div class="notice team-notice" role="status" data-team-open-notice><Icon name="info" size={16} /><span>{text()}</span></div>}</Show>
      <Show when={props.actionFailure}>{(reason) => <div class="notice danger team-notice" role="alert" data-team-action-failure><Icon name="triangle-alert" size={16} /><span>{reason()}</span></div>}</Show>
      <Show when={props.failure}>
        {(failure) => (
          <div class="notice danger team-notice" role="alert" data-team-failure>
            <Icon name="triangle-alert" size={16} />
            <span>{failure().code === "cursor_reset" ? failure().message : tr("Teams read failed: %1", failure().message)}</span>
          </div>
        )}
      </Show>
      <div class="team-body">
        <section class="team-main">
          <header class="team-board-head"><Icon name="columns-3" size={14} /><button type="button" class="btn small" aria-pressed={planMode() === "board"} onClick={() => setPlanMode("board")}>{tr("Board")}</button><button type="button" class="btn small" data-team-graph-mode aria-pressed={planMode() === "graph"} onClick={() => setPlanMode("graph")}>{tr("Graph")}</button><button type="button" class="btn small" data-team-lanes-mode aria-pressed={planMode() === "lanes"} onClick={() => setPlanMode("lanes")}>{tr("Lanes")}</button><button type="button" class="btn small" data-team-layers-mode aria-pressed={planMode() === "layers"} onClick={() => { setSelectedNode(null); setPlanMode("layers"); }}>{tr("Layers")}</button><span class="grow" /><button type="button" class="btn small team-members-toggle" data-team-members-toggle onClick={showMembers}><Icon name="users" size={12} />{tr("Members")}</button><button type="button" class="btn small" onClick={props.onAddTask}><Icon name="plus" size={12} />{tr("Add task")}</button></header>
          <div class="team-canvas" classList={{ "team-canvas-board": planMode() === "board" }}>
            <Show when={planMode() === "layers"} fallback={
              <Show when={planMode() === "lanes"} fallback={
                <Show when={planMode() === "graph"} fallback={
                  <div class="team-board" data-team-board onClick={dismissBlank}>
                    <Show when={all().length} fallback={<div class="empty team-empty"><Icon name="list-todo" size={24} /><h3>{tr("The queue is empty.")}</h3><p>{tr("Send a task to the team from the task board.")}</p></div>}>
                      <div class="team-board-columns">
                        <For each={groups()}>
                          {(group) => (
                            <section class="team-column" data-group={group.key}>
                              <h3><Show when={group.kind}><Icon name={group.kind === "review" ? "search-check" : group.kind === "test" ? "flask-conical" : "git-merge"} size={12} /></Show><span>{columnLabel(group)}</span><span class="count">{group.cards.length}</span></h3>
                              <div class="team-column-cards">
                                <Show when={!group.cards.length}><p class="team-column-empty">—</p></Show>
                                <For each={group.cards}>{(task) => <TeamTaskCard task={task} members={props.view.members} selected={selectedNode() === task.node} onSelect={() => select(task.node)} />}</For>
                      </div>
                    </section>
                  )}
                </For>
                      </div>
                    </Show>
                  </div>
                }>
                  <Show when={all().length} fallback={<div class="empty team-empty"><Icon name="list-todo" size={24} /><h3>{tr("The queue is empty.")}</h3><p>{tr("Send a task to the team from the task board.")}</p></div>}>
                    <PlanView view={props.view} busy={props.busy} selected={selectedNode()} onSelect={select} onSetDeps={(node, deps) => send("set_deps", { node, deps })} />
                  </Show>
                </Show>
              }>
                <LanesView view={props.view} selected={selectedNode()} onOpen={select} />
              </Show>
            }><PhasePane view={props.view} busy={props.busy} /></Show>
          </div>
          <div class="team-toolbar">
            <Show when={props.view.truncated}><span class="team-note">{tr("Some cards are not shown.")}</span></Show>
            <span class="grow" />
            <Show when={props.view.after}><button type="button" class="btn small" data-team-first-page onClick={() => props.onPage(null)}>{tr("First page")}</button></Show>
            <Show when={props.view.next}>
              {(cursor) => <button type="button" class="btn small" onClick={() => props.onPage(cursor())}>{tr("More…")}</button>}
            </Show>
          </div>
        </section>
        <aside ref={inspector} class="team-inspector" classList={{ "team-inspector-open": inspectorOpen() }} data-team-inspector
          data-team-inspector-overlay={narrow() && inspectorOpen() ? "" : undefined}
          role={narrow() && inspectorOpen() ? "dialog" : "complementary"}
          aria-label={card() ? tr("Task details") : tr("Members")}>
            <Show when={card()}>
              {(shown) => (
                <div class="team-task-detail" data-team-detail>
                  <header class="team-task-detail-head">
                    <h3>{shown().title || tr("Task %1", shown().node)}</h3>
                    <button type="button" class="btn small" data-team-inspector-members onClick={showMembers}>{tr("Members")}</button>
                    <button type="button" class="icon-btn" aria-label={tr("Close")} title={tr("Close")} onClick={dismissPreview}><Icon name="x" size={16} /></button>
                  </header>
                  <div class="team-task-detail-body">
                    <dl class="team-facts">
                      <dt>{tr("Task")}</dt><dd>
                        <Show when={shown().task} fallback={tr("not sent yet")}>
                          {(task) => <><span class="team-block">{task().project}</span><span class="team-block mono">{task().uuid}</span></>}
                        </Show>
                      </dd>
                      <dt>{tr("Role")}</dt><dd>{tr(shown().role || "unknown")}</dd>
                      <dt>{tr("Pipeline")}</dt><dd>{shown().pipeline || tr("none")}</dd>
                      <dt>{tr("Assigned to")}</dt><dd>{shown().assignee || tr("free")}</dd>
                      <Show when={shown().kind === "stage"} fallback={<><dt>{tr("Implemented by")}</dt><dd>{shown().implementer || tr("nobody yet")}</dd></>}>
                        <dt>{tr("Checks the work of")}</dt><dd>{shown().implementer || tr("nobody yet")}</dd>
                      </Show>
                      <dt>{tr("Round")}</dt><dd>{shown().round}</dd>
                      <Show when={shown().assigned_base}>{(base) => <><dt>{tr("Assigned base")}</dt><dd class="mono">{base()}</dd></>}</Show>
                      <Show when={shown().decided_by}><dt>{tr("Decided by")}</dt><dd>{tr(shown().decided_by)}</dd></Show>
                    </dl>
                    <Show when={shown().stages.length}>
                      <dl class="team-facts">
                        <For each={shown().stages}>{(stage) => (
                          <>
                            <dt aria-current={focusStage() === stage.node ? "true" : undefined} data-stage-focus={focusStage() === stage.node ? "" : undefined}>{tr("Stage %1", stage.index + 1)}</dt>
                            <dd data-stage-node={stage.node}><span class="team-block">{tr(stage.state)}</span><span class="team-block">{stageLine(stage)}</span>
                              <Show when={!stageAgrees(shown(), stage)}><span class="team-block" data-stage-inconsistent>{tr("Its facts disagree with this task.")}</span></Show></dd>
                          </>
                        )}</For>
                      </dl>
                    </Show>
                    <TaskDependencies task={shown()} status={props.view.bar.status} busy={props.busy} onSetDeps={(deps) => send("set_deps", { node: shown().node, deps })} />
                    <TaskActions card={shown()} teamStatus={props.view.bar.status} busy={props.busy} onAction={(kind) => send(kind, { node: shown().node })}
                      onReason={(button) => { setAsking(button); setReason(""); }} />
                    <Show when={asking()}>
                      {(button) => (
                        <div class="team-reason">
                          <TextInput class="field" aria-label={tr("Reason")} historyKey={`team-reason-${shown().node}`}
                            placeholder={button().id === "return" ? tr("What must change?") : tr("Why is it cancelled?")}
                            value={reason()} onInput={(event) => setReason(event.currentTarget.value)} />
                          <button type="button" class="btn small primary" disabled={!reason().trim()}
                            onClick={() => send(button().id, { node: shown().node, reason: reason() })}>{tr("Send")}</button>
                          <button type="button" class="btn small" onClick={() => setAsking(null)}>{tr("Cancel")}</button>
                        </div>
                      )}
                    </Show>
                  </div>
                </div>
              )}
            </Show>
            <section class="team-members" data-team-members hidden={!!card()} aria-label={tr("Members")}>
              <header class="team-section-head"><Icon name="users" size={14} /><h3>{tr("Members")}</h3><span class="count">{props.view.members.length}</span><span class="grow" /><button type="button" class="icon-btn team-members-close" data-team-members-close aria-label={tr("Close")} title={tr("Close")} onClick={dismissPreview}><Icon name="x" size={16} /></button></header>
              <For each={props.view.members}>
                {(member) => <MemberCard member={member} teamStatus={props.view.bar.status} cards={all()} busy={props.busy} onSelect={select}
                  onAction={(kind, id) => send(kind, { member: id })} />}
              </For>
            </section>
        </aside>
      </div>
    </div>
  );
}

/** The two exits from an uncertain action; nothing repeats it by itself. */
export function TeamUncertain(props: { detail: string; busy: boolean; onCheck: () => void; onDismiss: () => void }) {
  return (
    <div class="notice warn team-uncertain" role="alert" data-team-uncertain>
      <Icon name="triangle-alert" size={16} />
      <span>{props.detail}</span>
      <span class="team-finding-actions">
        <button type="button" class="btn small primary" disabled={props.busy} onClick={props.onCheck}>{tr("Check")}</button>
        <button type="button" class="btn small" disabled={props.busy} onClick={props.onDismiss}>{tr("Dismiss")}</button>
      </span>
    </div>
  );
}

/** The two store seams stay separate from rendering and following. */
function selectedLeadBinding() {
  const row = selected();
  if (!row) return { state: "none" as const };
  const opened = navigationTeamFor(row);
  if (opened) return { state: "lead" as const, team: opened };
  const identity = rowIdentity(row);
  // Without the computer's machine UUID no binding can be verified: no lookup.
  return identity.machine ? teams.teamOfSession(identity) : { state: "unknown" as const };
}
const actionReason = (source: string) => teams.failureFor(source)?.message;

/** Every unresolved call keeps its own identity and explicit Check/Dismiss. */
export function TeamUnresolvedList(props: { entries: readonly UnresolvedCall[]; busy: boolean; onCheck: (entry: UnresolvedCall) => void; onDismiss: (entry: UnresolvedCall) => void }) {
  return <For each={props.entries}>{(entry) => <section class="team-unresolved-entry" data-team-request={entry.call.request_id}>
    <div class="team-unresolved-name">{entry.call.team_name || entry.call.team}</div>
    <TeamUncertain detail={entry.detail} busy={props.busy} onCheck={() => props.onCheck(entry)} onDismiss={() => props.onDismiss(entry)} />
  </section>}</For>;
}

/** Selection and list changes rebind the tab; old reads settle before new ones. */
export function TeamTab() {
  const binding = createMemo(selectedLeadBinding);
  const target = createMemo(() => {
    const row = selected(), found = binding();
    return { key: JSON.stringify([row?.key, machineCards[row?.computer ?? ""]?.machine_id, row?.launchId, row?.runId]), team: found.state === "lead" ? found.team : undefined };
  }, undefined, { equals: (a, b) => a.key === b.key && a.team === b.team });
  const scope = followTeamScope(target, { close: () => teams.closeTeam(), open: (id) => teams.openTeam(id) });
  const sourceFor = (scope: TeamScope) => `team-tab:${scope.key}:${scope.team ?? ""}`;
  const source = () => sourceFor(target());
  createEffect(on(target, (_next, previous) => {
    if (previous) {
      teams.clearFailure(sourceFor(previous));
      if (teams.confirmation()?.call.team === previous.team) teams.cancelConfirmation();
    }
  }));
  onCleanup(() => { teams.clearFailure(source()); });
  const leadChanged = createMemo(() => {
    const row = selected(), current = target(), ready = scope.ready(), shown = teams.team();
    return !!row && !!shown && !!navigationTeamFor(row) && ready?.key === current.key && ready.team === current.team && shown?.bar.id === current.team
      && !rowIsSession(row, shown.members.find((member) => member.id === shown.bar.lead)?.session);
  });
  const view = createMemo(() => {
    const current = target(), ready = scope.ready(), shown = teams.team();
    return !leadChanged() && ready?.key === current.key && ready.team === current.team && shown?.bar.id === current.team ? shown : undefined;
  });
  const readFailure = () => target().team ? teams.readFailureFor(`team:${target().team}`) : null;
  const act: EditAction = (kind: string, fields: Record<string, unknown> = {}, revision?: number, team?: string) => {
    const current = view();
    if (!current || (team !== undefined && team !== current.bar.id)) return {
      outcome: "failed", request_id: "", detail: tr("The selected team changed. Reopen the form."),
    };
    const call: TeamCall = teamCall(current, newRequestId(), kind, fields);
    if (revision !== undefined) call.expected_revision = revision;
    return teams.run(call, source());
  };
  return <>
    <Show when={scope.failure()}>{(reason) => <div class="notice danger team-notice" role="alert"><Icon name="triangle-alert" size={16} /><span>{reason()}</span></div>}</Show>
    <Show when={view()} fallback={
      <div class="team-loading" role="status" data-team-waiting>
        <span class="dim">{leadChanged() ? tr("The team's lead session has changed. Open it from Teams again.") : binding().state === "none" ? tr("This session is not a team lead.") : binding().exhausted ? tr("Could not find this session's team here. Open it from Teams.") : tr("Finding the team of this session…")}</span>
      </div>
    }>{(current) => <TeamWorkspace view={current()} busy={teams.busy(current().bar.id)} failure={readFailure()} actionFailure={actionReason(source())}
      providerHome={teams.listed().find((row) => row.id === current().bar.id && row.project === current().bar.project)?.home}
      onSettingsSave={(call) => teams.run(call, source())}
      settingsBlocked={teams.unresolved().some((entry) => entry.call.team === current().bar.id && ["settings", "validation_config"].includes(entry.call.action.kind))}
      onAction={act} onPage={(cursor) => void teams.page(`team:${current().bar.id}`, cursor)} onAddTask={() => setPage("projects")} />}</Show>
    <Show when={!view() && actionReason(source())}>{(reason) => <div class="notice danger team-notice" role="alert" data-team-action-failure><Icon name="triangle-alert" size={16} /><span>{reason()}</span></div>}</Show>
    <TeamUnresolvedList entries={teams.unresolved()} busy={teams.busy()} onCheck={(entry) => void teams.check(entry, source())} onDismiss={(entry) => void teams.dismiss(entry, source())} />
    <TeamConfirmDialog source={source()} />
  </>;
}
