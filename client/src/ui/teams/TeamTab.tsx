import { For, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
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
import { Icon } from "../lib/icons";
import type { TeamView } from "./types";
import { boardColumns, stageAgrees, type BoardColumn } from "./pipeline-view";
import { followTeamScope, type TeamScope } from "./team-tab-scope";
import { TaskDependencies } from "./TaskDependencies";
import { PlanView } from "./PlanView";
import { TeamTaskCard } from "./TeamTaskCard";
import { TeamDrawerButtons, TeamDrawers } from "./TeamDrawers";
import type { TeamDrawer } from "../../store/teamDrawers";
import { openTeamNode, setOpenTeamNode } from "./openNode";
import "../styles/teams.css";

// The Team workspace combines the paged Board and dependency Graph with
// bounded Feed, Inbox, Reports and Cards drawers. Lead chat uses the bound
// lead's guarded composer. Views follow the store; actions use its intents.

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
  const [planMode, setPlanMode] = createSignal<"board" | "graph">("board");
  const [settingsView, setSettingsView] = createSignal<TeamView>();
  const [selectedNode, setSelectedNode] = createSignal<number | null>(null);
  const [drawer, setDrawer] = createSignal<TeamDrawer | null>(null);
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
  createEffect(on(teamId, () => { setSettingsView(undefined); setSelectedNode(null); setFocusStage(null); setOpenNotice(""); setAsking(null); setReason(""); lookups = 0; looked.clear(); }, { defer: true }));
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
  const select = (node: number) => { setSelectedNode(node); setFocusStage(null); setOpenNotice(""); };
  const card = createMemo(() => all().find((candidate) => candidate.node === selectedNode()) ?? null);
  const editors = createTeamEditors(() => props.view, props.onAction);
  const send = (kind: string, fields: Record<string, unknown> = {}) => { if (kind === "swap_member") { editors.open(kind, String(fields.member)); return; } props.onAction(kind, fields); setAsking(null); setReason(""); };
  return (
    <div class="team-tab" data-team-tab>
      <TeamBar view={props.view} busy={props.busy} onAction={(kind) => send(kind)}
        onSettings={props.onSettingsSave ? () => setSettingsView(props.view) : undefined} settingsBlocked={props.settingsBlocked}
        drawers={<TeamDrawerButtons value={drawer()} onOpen={(kind) => setDrawer(drawer() === kind ? null : kind)} />} />
      <Show when={settingsView()}><TeamSettingsDialog view={props.view} busy={props.busy || !!props.settingsBlocked}
        onSave={(call) => props.onSettingsSave!(call)} onClose={() => setSettingsView(undefined)} /></Show>
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
          <header class="team-board-head"><Icon name="columns-3" size={14} /><button type="button" class="btn small" aria-pressed={planMode() === "board"} onClick={() => setPlanMode("board")}>{tr("Board")}</button><button type="button" class="btn small" data-team-graph-mode aria-pressed={planMode() === "graph"} onClick={() => setPlanMode("graph")}>{tr("Graph")}</button><span class="grow" /><button type="button" class="btn small" onClick={props.onAddTask}><Icon name="plus" size={12} />{tr("Add task")}</button></header>
          <div class="team-canvas">
            <Show when={all().length} fallback={
              <div class="empty team-empty"><Icon name="list-todo" size={24} /><h3>{tr("The queue is empty.")}</h3><p>{tr("Send a task to the team from the task board.")}</p></div>
            }>
              <Show when={planMode() === "graph"} fallback={<div class="team-board" data-team-board>
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
              </div>}>
                <PlanView view={props.view} busy={props.busy} selected={selectedNode()} onSelect={select} onSetDeps={(node, deps) => send("set_deps", { node, deps })} />
              </Show>
            </Show>
            <Show when={card()}>
              {(shown) => (
                <div class="team-task-detail" data-team-detail>
                  <h3>{shown().title || tr("Task %1", shown().node)}</h3>
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
              )}
            </Show>
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
        <aside class="team-members" data-team-members aria-label={tr("Members")}>
          <header class="team-section-head"><Icon name="users" size={14} /><h3>{tr("Members")}</h3><span class="count">{props.view.members.length}</span></header>
          <For each={props.view.members}>
            {(member) => <MemberCard member={member} teamStatus={props.view.bar.status} cards={all()} busy={props.busy} onSelect={select}
              onAction={(kind, id) => send(kind, { member: id })} />}
          </For>
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
        <span class="dim">{leadChanged() ? tr("The team's lead session has changed. Open it from Teams again.") : binding().state === "none" ? tr("This session is not a team lead.") : tr("Finding the team of this session…")}</span>
      </div>
    }>{(current) => <TeamWorkspace view={current()} busy={teams.busy(current().bar.id)} failure={readFailure()} actionFailure={actionReason(source())}
      onSettingsSave={(call) => teams.run(call, source())}
      settingsBlocked={teams.unresolved().some((entry) => entry.call.team === current().bar.id && ["settings", "validation_config"].includes(entry.call.action.kind))}
      onAction={act} onPage={(cursor) => void teams.page(`team:${current().bar.id}`, cursor)} onAddTask={() => setPage("projects")} />}</Show>
    <Show when={!view() && actionReason(source())}>{(reason) => <div class="notice danger team-notice" role="alert" data-team-action-failure><Icon name="triangle-alert" size={16} /><span>{reason()}</span></div>}</Show>
    <TeamUnresolvedList entries={teams.unresolved()} busy={teams.busy()} onCheck={(entry) => void teams.check(entry, source())} onDismiss={(entry) => void teams.dismiss(entry, source())} />
    <TeamConfirmDialog source={source()} />
  </>;
}
