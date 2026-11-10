import { For, Match, Show, Suspense, Switch, createEffect, createMemo, createRoot, createResource, createSignal, lazy, on, onCleanup, untrack } from "solid-js";
import { tr } from "../../i18n";
import * as bridge from "../../bridge";
import { terminal } from "../../store/terminal";
import { NativeUiTab } from "./NativeUi";
import { nativeBinding, nativeEntry, nativeRoute } from "./native-entry";
import { nativeUi } from "../../store/native-ui";
import { pageHidden } from "../../store/visibility";
import { LeadProposals } from "../teams/ProposalCards";
import { teamAnswers } from "../teams/leadTeam";
import { selectedTeam } from "../../store/teams";
import { drops } from "../../store/drops";
import { processes, processesTab } from "../../store/processes";
import { flag } from "../../store/settings";
import type { TerminalStatus } from "../../types";
import { Icon } from "../lib/icons";
import { ActivityView, columnStyle } from "./Activity";
import { KeepWarmChip } from "./KeepWarmChip";
import { AgentBadge, MachineBadge, StatusBadge, captionText, headerState, statusOf, folderOf } from "./SessionCard";
import { detailTab, page, selected, setDetailTab, setMenu, subagentOpen, type MenuEntry } from "../../store/ui";
import { fallbackTab } from "./fallbackTab";
import { activity, subagentActivity } from "../../store/session";
import { activityReadOnly, type ActivitySource } from "../../store/activitySource";
import { archiveActivity } from "../../store/archiveActivity";
import { catalog, computerOffline } from "../../store/catalog";
import { copyIdentity, detailsFor, followedFor } from "../../store/actions";
import { headerClearSlot, sessionIdentity } from "./actions-ui";
import { actionEntries, copyEntries, freshActionEntry } from "./ActionDialogs";
import { snapshotOf } from "./actions-ui";
import { markEntry } from "./mark-entries";
import { followViewedReplies } from "./viewed-reply";
import { Composer } from "./Composer";
import { QueuePanel } from "./QueuePanel";
import { HeldInputBar, HeldMessage, TerminalDraftBar } from "./HeldMessage";
import { onCommand } from "../../store/shortcuts";
import { stepArea, stepTab } from "../../store/navigation";
import { composer } from "../../store/composer";
import { createSubagentIdentity } from "../../store/subagentIdentity";
import { selectedTask } from "../../store/taskNavigation";
import { followProjectTasks } from "../../store/taskContext";
import { inspectorOpen, inspectorTab, setInspectorOpen, openInspector, inspectorHint } from "../../store/inspector";
import { sessionGoal } from "./inspector-data";
import { PausedServiceDetail, servicePausedEmpty } from "../components/DesktopServiceStatus";

import { expandedEntries } from "./session-extra-entries";
import { folderEntries, folderRequest } from "../../store/sessionFolders";
import { watchDetailRemoval } from "./detail-removal";
import { lazyView } from "../lazy-view";
import { LazyBoundary } from "../lazy-boundary";
import { ComposerDialogsSlot, QuestionSlot } from "./lazy-slots";

// Task #61 (team ede724c4; CI #22: the entry chunk was 559 KB of its 500 KB
// budget, 324 KB of it the Teams UI): the Team tab, the lead's team header and
// badge, the New team page and the waiting banner load on first use.
const TeamTab = lazyView(() => import("../teams/TeamTab"), (m) => m.TeamTab);
const LeadTeamHeader = lazyView(() => import("../teams/TeamTab"), (m) => m.LeadTeamHeader);
const LeadTitleBadge = lazyView(() => import("../teams/TeamTab"), (m) => m.LeadTitleBadge);
const NewTeamPage = lazyView(() => import("../teams/NewTeamPage"), (m) => m.NewTeamPage);
const WaitingBanner = lazyView(() => import("../teams/Waiting"), (m) => m.WaitingBanner);
// Opened on demand, outside the entry chunk and preloaded on idle (zerus-ycycu).
const ProcessesTab = lazyView(() => import("./Processes"), (m) => m.ProcessesTab);
const TerminalTab = lazyView(() => import("./Terminal"), (m) => m.TerminalTab);

const Inspector = lazy(() => import("./Inspector").then((m) => ({ default: m.Inspector })));
const TaskView = lazy(() => import("../screens/TaskView"));
let reportedRemovedDetail = false;

// The session header and its Activity with the composer under it, after
// design/webui SessionDetail. Session actions keep the mockup placement and
// use the existing guarded handlers.

function headerCaption(): string {
  const s = selected();
  if (!s) return "";
  return statusOf(s) === "offline" ? tr("Offline") : captionText(s.caption);
}

function EmptyDetail() {
  return (
    <Show when={!servicePausedEmpty()} fallback={<PausedServiceDetail />}>
      <div class="detail-empty">
        <h2>{tr("A little more focus.")}</h2>
        <p>{tr("Select a session to see what your agent is doing.")}<br />{tr("New sessions appear here automatically.")}</p>
      </div>
    </Show>
  );
}

function Header() {
  const s = () => selected()!;
  const tone = () => (statusOf(s()) === "attention" ? "attention" : statusOf(s()));
  // Details come from the followed session's Activity state value; the store
  // inspects only once that subscription answered without one.
  const details = createMemo(() => detailsFor(s(), !computerOffline(s().computer), followedFor(s().key)));
  const childDetails = createSubagentIdentity(bridge.inspectSubagentIdentity);
  createEffect(() => {
    const id = subagentOpen();
    const row = s(), d = details();
    childDetails.follow(id ? { agentId: id, archiveId: row.archiveId ?? null,
      target: { computer: row.computer, name: row.name, run_id: d?.run_id || row.runId || "", conversation_id: d?.conversation_id || row.conversationId || "" } } : undefined);
  });
  onCleanup(() => childDetails.follow(undefined));
  const child = () => childDetails.child() ?? (subagentOpen() ? { id: subagentOpen()! } : undefined);
  const identity = () => sessionIdentity(s(), details(), child());
  const folderScope = createMemo(() => JSON.stringify(folderRequest(s(), details(), child())));
  const [folderStatus] = createResource(folderScope, async (scope) => {
    const request = JSON.parse(scope) as ReturnType<typeof folderRequest>;
    return { scope, status: await bridge.sessionFolderStatus(request.target.computer, request.directory).catch(() => undefined) };
  });
  const folders = () => folderEntries(s(), folderRequest(s(), details(), child()), folderStatus()?.scope === folderScope() ? folderStatus()?.status : undefined);

  const copyId = () => { void copyIdentity(snapshotOf(s()), details(), child()); };
  const act = createMemo(() => actionEntries(s(), details()));
  const copies = createMemo(() => copyEntries(s(), details()));
  const mark = createMemo(() => markEntry(s()));
  const slot = () => headerClearSlot(!subagentOpen(), s().state);
  const fresh = createMemo(() => freshActionEntry(s(), true));
  // Clear session goes through the composer's clear_context (Qt's clearContext): its dialog shows the identity it sends back.
  const clear = () => composer.view()?.clear;
  const clearEntry = (): MenuEntry => ({
    label: tr("Clear session…"), icon: "clear", disabled: !clear()?.enabled, hint: clear()?.reason || undefined, action: () => composer.openClear(),
  });
  const actions = (): MenuEntry[] => [
    folders().folder,
    folders().shell,
    { label: identity().caption, icon: "copy", disabled: !identity().value, action: copyId },
    { separator: true },
    act().rename,
    act().fork,
    { separator: true },
    copies().command,
    copies().name,
    copies().reference,
    copies().folder,
    ...expandedEntries(s()),
    ...(nativeEntry(s()) ? [nativeEntry(s())!] : []),
    act().change,
    ...(mark() ? [mark()!] : []),
    { separator: true },
    ...(slot() === "fresh" && fresh() ? [fresh()!] : slot() === "clear" ? [clearEntry()] : []),
    ...(act().archive ? [act().archive!] : []),
    act().end,
  ];
  return (
    <header class="detail-head">
      <div class="dh-row1">
        <h1 class="ell" title={s().name}>{s().project} / {s().title}<Show when={subagentOpen()}> / {s().subagents?.find((k) => k.id === subagentOpen())?.name}</Show></h1>
        <LazyBoundary><LeadTitleBadge /></LazyBoundary>
        <Show when={s().computer !== "" && statusOf(s()) === "working" && !activity.loaded()}><span class="busy-arc" title={tr("Waiting for the remote read")} /></Show>
        <span class="grow" />
        {/* Task 315 (F3): the host's state, as the card shows it (Qt's header pill). */}
        <Show when={headerState(s())} fallback={<span class={`hbadge t-${tone()}`}>{headerCaption()}</span>}>
          {(state) => <span class="dh-state"><StatusBadge kind={state().kind} caption={state().caption} title={state().title} /></span>}
        </Show>
        <button class="icon-btn" data-tip={folders().folder.hint} aria-label={tr("Open session folder in file manager")} disabled={folders().folder.disabled} onClick={() => folders().folder.action?.()}><Icon name="folder-open" size={16} /></button>
        <button class="icon-btn" data-tip={tr("Open shell in session folder")} aria-label={tr("Open shell in session folder")} disabled={folders().shell.disabled} onClick={() => folders().shell.action?.()}><Icon name="square-terminal" size={16} /></button>
        <button class="icon-btn" data-tip={identity().caption} aria-label={identity().caption} disabled={!identity().value} onClick={copyId}><Icon name="copy" size={15} /></button>
        <Show when={slot() === "clear"}>
          <button class="icon-btn" data-tip={clear()?.reason || tr("Clear session")}
            aria-label={clear()?.running ? tr("Clearing…") : tr("Clear session")} disabled={!clear()?.enabled} onClick={() => composer.openClear()}><Icon name="clear" size={15} /></button>
        </Show>
        <Show when={!subagentOpen() && (s().status === "ended" || s().state === "archived")}><button class="btn small primary" title={act().change.hint} disabled={act().change.disabled} onClick={() => act().change.action()}><Icon name="play" size={12} />{tr("Restore")}</button></Show>
        <button class="icon-btn" data-tip={tr("Session actions")} aria-label={tr("Session actions")} onClick={(e) => setMenu({ x: e.clientX - 200, y: e.clientY + 12, items: actions() })}><Icon name="ellipsis" size={16} /></button>
      </div>
      <div class="dh-row2">
        <AgentBadge agent={s().agent} />
        <MachineBadge alias={s().machine} computer={s().computer} />
        {/* The branch is a label beside the agent and machine badges, with the branch glyph. */}
        <Show when={s().branch}>
          <span class="dh-branch" title={tr("Git branch %1", s().branch!)}><Icon name="git-branch" size={12} /><span class="ell">{s().branch}</span></span>
        </Show>
        <Show when={s().cwd}>
          <span class="dh-chip ell" title={s().cwd}><Icon name="folder" size={13} />{folderOf(s())}</span>
        </Show>
        <Show when={s().worktree}><span class="dh-chip ell" title={tr("Linked worktree %1", s().worktree!)}><Icon name="folder-git-2" size={13} />{tr("worktree %1", s().worktree!)}</span></Show>
        <span class="grow" />
        <KeepWarmChip session={s()} />
      </div>
    </header>
  );
}

type Tab = { id: string; label: string };
const TABS: readonly Tab[] = [{ id: "activity", label: "Activity" }, { id: "terminal", label: "Terminal" }];
/** Task #331: the strip renders these fixed ids, never a fresh tab object, so
 * a status poll, a team read or an unread change cannot remount a button and
 * restart the active tab's underline animation. */
export const TAB_ORDER = ["activity", "terminal", "native", "processes", "taskview", "team", "newteam"] as const;

/** The selected child's bounded page, an archive's recorded page, or the live parent follow. */
export const activitySourceFor = (key: string): ActivitySource => (subagentOpen() ? subagentActivity : activityReadOnly(key) ? archiveActivity : activity);

/** The tabs of a row: an archived row has no Terminal (Qt hides only that one; terminal_status answers unsupported). */
export function detailTabs(key: string): readonly Tab[];
export function detailTabs<T extends Tab>(key: string, tabs: readonly T[]): readonly T[];
export function detailTabs(key: string, tabs: readonly Tab[] = TABS): readonly Tab[] {
  return activityReadOnly(key) ? tabs.filter((t) => t.id !== "terminal") : tabs;
}

/** The selected session's terminal status (terminal_status), refreshed when its inputs change. */
const [terminalTexts, setTerminalTexts] = createSignal<TerminalStatus | undefined>();
createRoot(() => {
  createEffect(() => {
    const s = selected(), binding = nativeBinding(s), online = !!s && s.state !== "archived" && !computerOffline(s.computer);
    untrack(() => nativeUi.select(binding, online));
  });
  createEffect(() => {
    const shown = page() === "sessions" && detailTab() === "native" && !subagentOpen() && !pageHidden();
    untrack(() => nativeUi.opened(shown));
  });
  // The inputs of terminal_status as one string: a row change that leaves them alone calls nothing.
  const inputs = createMemo(() => {
    const s = selected();
    if (!s) return undefined;
    const followed = followedFor(s.key);
    const details = detailsFor(s, !computerOffline(s.computer), followed);
    return JSON.stringify([s.key, s.name, s.runId, s.cmd, s.state, s.terminalSupported, s.terminalReported, s.terminalAttachSupported,
      s.terminalAttachReported, !computerOffline(s.computer), followed.value ?? details ?? null]);
  });
  // Another selection: input stops reaching the previous session before anything is awaited.
  createEffect(on(() => selected()?.key, () => { terminal.leaveSession(); }, { defer: true }));
  createEffect(on(inputs, (input) => {
    const s = selected();
    if (!input || !s) { setTerminalTexts(undefined); void terminal.setTarget(undefined, undefined); return; }
    const followed = followedFor(s.key);
    void bridge.terminalStatus({
      facts: { cmd: s.cmd, terminal_supported: s.terminalSupported, terminal_reported: s.terminalReported, terminal_attach_supported: s.terminalAttachSupported,
        terminal_attach_reported: s.terminalAttachReported, terminal_reason: "", terminal_attach_reason: "" },
      details: followed.value ?? detailsFor(s, !computerOffline(s.computer), followed) ?? {},
      state: s.state, online: !computerOffline(s.computer), restored_pending: false, name: s.name, machine: s.machine,
    }).then((status) => {
      if (selected()?.key !== s.key) return;
      setTerminalTexts(status);
      void terminal.setTarget({ computer: s.computer, name: s.name, run_id: s.runId ?? "", machine: s.machine }, status);
    });
  }));
  // Leaving the Sessions page disconnects, and coming back does not connect (SessionsWindow.cpp:1637, QStackedWidget::currentChanged).
  createEffect(on(page, (p) => { if (p !== "sessions") void terminal.leavePage(); }, { defer: true }));
});

/** Task #361: the tab row's entries; the strip renders their ids in TAB_ORDER. */
export const tabs = () => {
  const count = processes.view()?.count ?? 0;
  const all = [
    { id: "activity" as const, label: tr("Activity"), show: true },
    { id: "terminal" as const, label: tr("Terminal"), show: !!terminalTexts()?.supported && !subagentOpen() },
    { id: "native" as const, label: tr("Native UI"), show: selected()?.agent === "deepseek" && !subagentOpen() },
    processesTab(flag("processes/enabled"), count),
    { id: "taskview" as const, label: tr("Task View"), show: detailTab() === "taskview" && !!selectedTask() },
    // Task 9: the selected session's team (its lead's or a member's session).
    // Task 200 (Qt F5): a draft is not a team yet; its lead's tab is New team, which shows the draft again.
    { id: "team" as const, label: selectedTeam()?.status === "draft" ? tr("New team") : tr("Team"), show: !!selectedTeam() && !subagentOpen() },
    // Task #326 (N2): Qt also offers "+ New team" on a session that has no team.
    { id: "newteam" as const, label: tr("+ New team"), show: !selectedTeam() && !subagentOpen() && selected()?.computer === "" && selected()?.state !== "archived" },
  ];
  const s = selected();
  return s ? detailTabs(s.key, all) : all;
};

const visible = (el: Element | null): el is HTMLElement => !!el && (el as HTMLElement).offsetParent !== null && !(el as HTMLElement).hidden;
const FOCUSABLE = "textarea:not([hidden]):not([disabled]), input:not([disabled]), button:not([disabled]), [tabindex]:not([tabindex='-1']), select:not([disabled])";
/** What focus lands on in an area: its first focusable element, else the area itself. */
function focusInto(area: HTMLElement | null) {
  if (!area) return;
  const target = [...area.querySelectorAll<HTMLElement>(FOCUSABLE)].find(visible);
  if (target) { target.focus(); return; }
  if (!area.hasAttribute("tabindex")) area.tabIndex = -1;
  area.focus();
}
/** The current view's focus target (Qt viewFocusTarget): Activity's message field or its scroll, another tab's first control. */
function focusView() {
  requestAnimationFrame(() => {
    const editor = document.querySelector<HTMLElement>(".detail .tab-body .cmp-editor");
    if (detailTab() === "activity" && visible(editor)) { editor.focus(); return; }
    focusInto(document.querySelector<HTMLElement>(".detail .tab-body"));
  });
}

/** Task 242 (Qt stepTab): the next shown tab of the selected session, wrapping. */
function stepDetailTab(direction: 1 | -1) {
  if (page() !== "sessions" || !selected()) return;
  const next = stepTab(tabs(), detailTab(), direction);
  if (!next) return;
  if (next === "terminal") terminal.opened();
  setDetailTab(next);
  focusView();
}

/** Task 242 (Qt tab.terminal): Terminal and back to Activity's message field. */
function toggleTerminal() {
  if (page() !== "sessions" || !selected()) return;
  if (detailTab() !== "terminal") {
    if (!tabs().find((tab) => tab.id === "terminal")?.show) return;
    terminal.opened();
    setDetailTab("terminal");
    requestAnimationFrame(() => focusInto(document.querySelector<HTMLElement>(".detail .tab-body")));
  } else {
    setDetailTab("activity");
    focusView();
  }
}

/**
 * Task 242 (Qt stepFocus, F6): the Sessions page's areas in order, the session
 * list, the current view, the message field and the side panel. The innermost
 * area holding focus is the current one (the message field lies in Activity).
 */
function stepFocusArea(direction: 1 | -1) {
  if (page() !== "sessions") return;
  const areas: { area: HTMLElement; focus: () => void }[] = [];
  const list = document.querySelector<HTMLElement>(".sessions-pane .sp-scroll");
  if (visible(list)) areas.push({ area: list, focus: () => list.focus() });
  const view = document.querySelector<HTMLElement>(".detail .tab-body");
  if (selected() && visible(view)) areas.push({ area: view, focus: () => focusInto(view) });
  const editor = document.querySelector<HTMLElement>(".detail .tab-body .cmp-editor");
  if (visible(editor) && !(editor as HTMLTextAreaElement).disabled) areas.push({ area: editor.closest<HTMLElement>(".cmp-col") ?? editor, focus: () => editor.focus() });
  const inspector = document.querySelector<HTMLElement>(".detail .inspector");
  if (visible(inspector)) areas.push({ area: inspector, focus: () => focusInto(inspector.querySelector<HTMLElement>(".insp-scroll, .insp-body") ?? inspector) });
  const active = document.activeElement;
  let current = -1;
  areas.forEach((entry, index) => { if (active && entry.area.contains(active)) current = index; });
  const next = stepArea(areas.length, current, direction);
  if (next >= 0) areas[next].focus();
}

createRoot(() => createEffect(() => {
  const next = fallbackTab(tabs(), detailTab());
  if (next) setDetailTab(next);
}));

export function SessionDetail() {
  followProjectTasks();
  // Owner, 2026-11-09: Start planning / Create team give the session its team
  // (a draft first); the "+ New team" tab then hides, and the view used to fall
  // back to Activity. It follows the team to the Team tab instead.
  createEffect(() => {
    if (detailTab() === "newteam" && selectedTeam()) setDetailTab("team");
  });
  // A reply shown in Activity or Terminal for a second is read (Qt's checkViewedReply).
  followViewedReplies();
  const [available, setAvailable] = createSignal(900);
  // One lifetime for the whole frame. Archive browsing has its own bounded
  // row store; every live detail must still belong to the current catalog.
  const shown = createMemo(() => {
    const row = selected();
    return row && (activityReadOnly(row.key) || catalog.rows[row.key]) ? row : undefined;
  });
  let frame: HTMLDivElement | undefined;
  const removal = watchDetailRemoval({
    listed: (key) => !!catalog.rows[key],
    schedule: (check) => { const id = requestAnimationFrame(check); return () => cancelAnimationFrame(id); },
    report: (line) => {
      if (reportedRemovedDetail) return;
      reportedRemovedDetail = true;
      void bridge.reportDetailInvariant(line);
    },
  });
  createEffect(() => {
    const row = shown();
    removal.update(row && frame ? {
      key: row.key, catalogBound: !activityReadOnly(row.key), sessionId: row.sessionId, element: frame,
      route: { tab: detailTab(), inspector: inspectorOpen() ? inspectorTab() : "closed", child: !!subagentOpen() },
    } : undefined);
  });
  onCleanup(() => removal.dispose());
  const measure = (el: HTMLDivElement) => {
    const observer = new ResizeObserver(() => setAvailable(el.clientWidth)); observer.observe(el);
    onCleanup(() => observer.disconnect());
  };
  const goal = createMemo(() => sessionGoal(selected() ? followedFor(selected()!.key).value : undefined));
  const signIn = createMemo(() => {
    const s = selected();
    return s?.agent === "deepseek" && s.state === "running" && !computerOffline(s.computer)
      && detailsFor(s, true, followedFor(s.key))?.auth_required;
  });
  const toggle = onCommand("panel.toggle", () => { if (page() === "sessions" && selected()) setInspectorOpen(!inspectorOpen()); });
  onCleanup(toggle);
  // Task 242 (Qt stepTab, tab.terminal, stepFocus): Ctrl+Tab, Ctrl+backtick and F6.
  const offTabs = [
    onCommand("tab.next", () => stepDetailTab(1)),
    onCommand("tab.previous", () => stepDetailTab(-1)),
    onCommand("tab.terminal", toggleTerminal),
    onCommand("focus.next", () => stepFocusArea(1)),
    onCommand("focus.previous", () => stepFocusArea(-1)),
  ];
  onCleanup(() => offTabs.forEach((undo) => undo()));
  createEffect(() => {
    if (page() !== "sessions" || !selected()) return;
    // Window scope on every detail tab (SessionsWindowShortcuts.cpp:202,242).
    const stop = onCommand("queue.sendNow", () => { void composer.sendNow(); });
    onCleanup(stop);
  });
  return (
    <section class="detail" aria-label={tr("Session")}>
      {/* Qt's drop overlay (SessionFileDrop): what a drop on this panel does. */}
      <Show when={drops.hint()}>{(text) => <div class="drop-overlay" aria-hidden="true"><span>{text()}</span></div>}</Show>
      <Show when={shown()} fallback={<div class="work"><div class="work-main"><EmptyDetail /></div></div>}>
        <div class="detail-frame" ref={frame}>
          <Header />
          <div class="work" ref={measure}>
            <div class="work-main">
              {/* Task 317 (F13): the lead's session carries its team bar and strip as Qt's
                  lead header does; the Team tab has its own bar. */}
              <Show when={detailTab() !== "team"}><LazyBoundary><LeadTeamHeader /></LazyBoundary></Show>
              <SessionTabRow />
              <div class="tab-body">
                <Switch>
                  <Match when={detailTab() === "activity"}>
                    <div class="activity-wrap" classList={{ "has-goal": !!goal().objective && !subagentOpen() }} style={columnStyle()}>
                      {/* Task 116: what waits for the owner, in the lead's Activity
                          strip too (Qt's waitingBannerHost): every kind the team
                          feed has, each answered in its row as Waiting for you
                          answers it, and one click to its team or the full list. */}
                      <LazyBoundary><WaitingBanner /></LazyBoundary>
                      <Show when={goal().objective && !subagentOpen()}><button class="goal-pill" onClick={() => openInspector("plan")}><Icon name="circle-dot" size={13} /><b>{goal().caption}</b><span class="ell">{goal().objective}</span><span class="grow" /><span>{tr("Details")}</span><Icon name="chevron-right" size={12} /></button></Show>
                      <ActivityView source={activitySourceFor(selected()!.key)} />
                      {/* Child Activity stays read-only; its own composer opens only when the inspection supports send. */}
                      <Show when={!subagentOpen()}>
                        <Show when={signIn()}><div class="cmp-col"><div class="qcard">
                          <p class="qcard-body">{tr("Sign in to DeepSeek to start using this session. Complete sign-in in the official DeepSeek UI, then return here.")}</p>
                          <div class="qcard-actions"><button class="btn small" onClick={() => nativeRoute(selected())()}>{tr("Sign in to DeepSeek…")}</button></div>
                        </div></div></Show>
                        <HeldMessage /><QueuePanel /><LeadProposals /><QuestionSlot /><HeldInputBar /><TerminalDraftBar />
                      </Show>
                      <div class="cmp-col"><Composer /></div>
                    </div>
                  </Match>
                  <Match when={detailTab() === "taskview"}><Suspense><TaskView /></Suspense></Match>
                  <Match when={detailTab() === "terminal"}><LazyBoundary loading={<div class="lazy-waiting">{tr("Loading…")}</div>}><TerminalTab /></LazyBoundary></Match>
                  <Match when={detailTab() === "native"}><NativeUiTab /></Match>
                  <Match when={detailTab() === "processes"}><LazyBoundary loading={<div class="lazy-waiting">{tr("Loading…")}</div>}><ProcessesTab /></LazyBoundary></Match>
                  <Match when={detailTab() === "team" && selectedTeam()?.status === "draft" && selected()!.computer === "" && selectedTeam()}>{(team) => <LazyBoundary><NewTeamPage embedded draft={team().id} /></LazyBoundary>}</Match>
                  <Match when={detailTab() === "team" && selectedTeam()}>{(team) => <LazyBoundary><TeamTab computer={selected()!.computer} team={team().id} /></LazyBoundary>}</Match>
                  <Match when={detailTab() === "newteam" && !selectedTeam() && selected()?.computer === ""}><LazyBoundary><NewTeamPage embedded /></LazyBoundary></Match>
                </Switch>
              </div>
            </div>
          <Suspense><Inspector available={available()} /></Suspense>
          </div>
        </div>
      </Show>
        {/* The composer's questions and Clear session, also from the header on another tab; the field takes the focus back. */}
        <ComposerDialogsSlot onClosed={() => { if (document.activeElement === document.body) document.querySelector<HTMLTextAreaElement>(".cmp-editor")?.focus(); }} />
    </section>
  );
}

/**
 * Task #426 (owner, screenshot of package 167): the session view's tab row. The
 * skin's tab spacing lives here as one component so a render test pins it: each
 * tab carries the standard padding and the row a real gap between the tabs, the
 * team's "?N" badge stands apart from its label (the tab's own gap plus a small
 * margin), and the active underline stays under its own tab. The label is its
 * own span so a narrow window shortens it with an ellipsis instead of letting
 * the tabs run into each other; the row scrolls rather than squeezing them.
 */
export function SessionTabRow() {
  return (
    <div class="detail-tabs tabs" role="tablist">
      <For each={TAB_ORDER}>{(id) => {
        const tab = () => tabs().find((found) => found.id === id);
        return (
          <button class="tab" role="tab" data-tab={id} data-tab-id={id} hidden={!tab()?.show} classList={{ on: detailTab() === id }}
            aria-selected={detailTab() === id} onClick={() => { if (id === "terminal") terminal.opened(); setDetailTab(id); }}>
            <span class="tab-label">{tab()?.label}</span>
            <Show when={id === "team" && teamAnswers(selectedTeam()) > 0}><span class="tab-count" data-tip={tr("Needs your answer")}>?{teamAnswers(selectedTeam())}</span></Show>
          </button>
        );
      }}</For>
      <span class="grow" />
      <Show when={!inspectorOpen()}><button class="icon-btn small" data-cmd="show-side-panel" aria-label={tr("Show side panel")} data-tip={inspectorHint(tr("Show tasks, plan, agents, details and worktrees"))} data-tip-pos="left" onClick={() => setInspectorOpen(true)}><Icon name="panel-right" size={15} /></button></Show>
    </div>
  );
}
