import { For, Match, Show, Switch, createEffect, createMemo, createRoot, createSignal, on, onCleanup } from "solid-js";
import * as bridge from "../../bridge";
import { terminal } from "../../store/terminal";
import { TerminalTab } from "./Terminal";
import { ProcessesTab } from "./Processes";
import { drops } from "../../store/drops";
import { processes, processesTab } from "../../store/processes";
import { flag } from "../../store/settings";
import type { TerminalStatus } from "../../types";
import { Icon } from "../lib/icons";
import { ActivityView, columnStyle } from "./Activity";
import { AgentBadge, MachineBadge, statusOf, folderOf } from "./SessionCard";
import { detailTab, page, selected, setDetailTab, setMenu, subagentOpen, type MenuEntry } from "../../store/ui";
import { activity } from "../../store/session";
import { activityReadOnly, type ActivitySource } from "../../store/activitySource";
import { archiveActivity } from "../../store/archiveActivity";
import { computerOffline } from "../../store/catalog";
import { copyIdentity, detailsFor, followedFor } from "../../store/actions";
import { headerClearSlot, sessionIdentity } from "./actions-ui";
import { actionEntries, freshActionEntry } from "./ActionDialogs";
import { snapshotOf } from "./actions-ui";
import { markEntry } from "./mark-entries";
import { Composer } from "./Composer";
import { ComposerDialogs } from "./ComposerDialogs";
import { QuestionCard } from "./QuestionCard";
import { QueuePanel } from "./QueuePanel";
import { onCommand } from "../../store/shortcuts";
import { composer } from "../../store/composer";
import { createSubagentIdentity } from "../../store/subagentIdentity";
import { selectedTask } from "../../store/taskNavigation";
import { Inspector } from "./Inspector";
import { TaskView, followProjectTasks } from "../screens/Tasks";
import { inspectorOpen, setInspectorOpen, openInspector, inspectorHint } from "../../store/inspector";
import { sessionGoal } from "./inspector-data";

// The session header and its Activity with the composer under it, after
// design/webui SessionDetail. Session actions keep the mockup placement and
// use the existing guarded handlers.

function headerCaption(): string {
  const s = selected();
  if (!s) return "";
  return statusOf(s) === "offline" ? "Offline" : s.caption;
}

function EmptyDetail() {
  return (
    <div class="detail-empty">
      <h2>A little more focus.</h2>
      <p>Select a session to see what your agent is doing.<br />New sessions appear here automatically.</p>
    </div>
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
  const child = childDetails.child;
  const identity = () => sessionIdentity(s(), details(), child());
  const copyId = () => { void copyIdentity(snapshotOf(s()), details(), child()); };
  const act = createMemo(() => actionEntries(s(), details()));
  const mark = createMemo(() => markEntry(s()));
  const slot = () => headerClearSlot(!subagentOpen(), s().state);
  const fresh = createMemo(() => freshActionEntry(s(), true));
  // Clear session goes through the composer's clear_context (Qt's clearContext): its dialog shows the identity it sends back.
  const clear = () => composer.view()?.clear;
  const clearEntry = (): MenuEntry => ({
    label: "Clear session…", icon: "clear", disabled: !clear()?.enabled, hint: clear()?.reason || undefined, action: () => composer.openClear(),
  });
  const actions = (): MenuEntry[] => [
    { label: "Open folder", icon: "folder-open", disabled: true },
    { label: "Open shell in session folder", icon: "square-terminal", disabled: true },
    { label: identity().caption, icon: "copy", disabled: !identity().value, action: copyId },
    { separator: true },
    act().rename,
    { label: "Fork session…", icon: "git-fork", disabled: true },
    { separator: true },
    { label: "Copy connect command", icon: "link", disabled: true },
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
        <Show when={s().computer !== "" && statusOf(s()) === "working" && !activity.loaded()}><span class="busy-arc" title="Waiting for the remote read" /></Show>
        <span class="grow" />
        <span class={`hbadge t-${tone()}`}>{headerCaption()}</span>
        <button class="icon-btn" data-tip="Open session folder in file manager" disabled><Icon name="folder-open" size={16} /></button>
        <button class="icon-btn" data-tip="Open shell in session folder" disabled><Icon name="square-terminal" size={16} /></button>
        <button class="icon-btn" data-tip={identity().caption} aria-label={identity().caption} disabled={!identity().value} onClick={copyId}><Icon name="copy" size={15} /></button>
        <Show when={slot() === "clear"}>
          <button class="icon-btn" data-tip={clear()?.reason || "Clear session"}
            aria-label={clear()?.running ? "Clearing…" : "Clear session"} disabled={!clear()?.enabled} onClick={() => composer.openClear()}><Icon name="clear" size={15} /></button>
        </Show>
        <Show when={!subagentOpen() && (s().status === "ended" || s().state === "archived")}><button class="btn small primary" title={act().change.hint} disabled={act().change.disabled} onClick={() => act().change.action()}><Icon name="play" size={12} />Restore</button></Show>
        <button class="icon-btn" data-tip="Session actions" onClick={(e) => setMenu({ x: e.clientX - 200, y: e.clientY + 12, items: actions() })}><Icon name="ellipsis" size={16} /></button>
      </div>
      <div class="dh-row2">
        <AgentBadge agent={s().agent} />
        <MachineBadge alias={s().machine} computer={s().computer} />
        {/* The branch is a label beside the agent and machine badges, with the branch glyph. */}
        <Show when={s().branch}>
          <span class="dh-branch" title={`Git branch ${s().branch}`}><Icon name="git-branch" size={12} /><span class="ell">{s().branch}</span></span>
        </Show>
        <Show when={s().cwd}>
          <span class="dh-chip ell" title={s().cwd}><Icon name="folder" size={13} />{folderOf(s())}</span>
        </Show>
        <Show when={s().worktree}><span class="dh-chip ell" title={`Linked worktree ${s().worktree}`}><Icon name="folder-git-2" size={13} />worktree {s().worktree}</span></Show>
        <span class="grow" />
        <span class="dh-model ell">{s().model} {s().effort ?? ""}</span>
      </div>
    </header>
  );
}

type Tab = { id: string; label: string };
const TABS: readonly Tab[] = [{ id: "activity", label: "Activity" }, { id: "terminal", label: "Terminal" }];

/** The Activity of a row: an archived row's recorded one, read-only and never streamed, else the live follow. */
export const activitySourceFor = (key: string): ActivitySource => (activityReadOnly(key) ? archiveActivity : activity);

/** The tabs of a row: an archived row has no Terminal (Qt hides only that one; terminal_status answers unsupported). */
export function detailTabs(key: string): readonly Tab[];
export function detailTabs<T extends Tab>(key: string, tabs: readonly T[]): readonly T[];
export function detailTabs(key: string, tabs: readonly Tab[] = TABS): readonly Tab[] {
  return activityReadOnly(key) ? tabs.filter((t) => t.id !== "terminal") : tabs;
}

/** The selected session's terminal status (terminal_status), refreshed when its inputs change. */
const [terminalTexts, setTerminalTexts] = createSignal<TerminalStatus | undefined>();
createRoot(() => {
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

const tabs = () => {
  const count = processes.view()?.count ?? 0;
  const all = [
    { id: "activity" as const, label: "Activity", show: true },
    { id: "terminal" as const, label: "Terminal", show: !!terminalTexts()?.supported && !subagentOpen() },
    processesTab(flag("processes/enabled"), count),
    { id: "taskview" as const, label: "Task View", show: detailTab() === "taskview" && !!selectedTask() },
  ];
  const s = selected();
  return s ? detailTabs(s.key, all) : all;
};

// A tab that disappears returns to Activity (SessionsWindow.cpp:4019-4020).
createRoot(() => createEffect(() => { if (!tabs().find((t) => t.id === detailTab())?.show) setDetailTab("activity"); }));

export function SessionDetail() {
  followProjectTasks();
  const [available, setAvailable] = createSignal(900);
  const measure = (el: HTMLDivElement) => {
    const observer = new ResizeObserver(() => setAvailable(el.clientWidth)); observer.observe(el);
    onCleanup(() => observer.disconnect());
  };
  const goal = createMemo(() => sessionGoal(selected() ? followedFor(selected()!.key).value : undefined));
  const toggle = onCommand("panel.toggle", () => { if (page() === "sessions") setInspectorOpen(!inspectorOpen()); });
  onCleanup(toggle);
  createEffect(() => {
    if (page() !== "sessions" || !selected()) return;
    // Window scope on every detail tab (SessionsWindowShortcuts.cpp:202,242).
    const stop = onCommand("queue.sendNow", () => { void composer.sendNow(); });
    onCleanup(stop);
  });
  return (
    <section class="detail" aria-label="Session">
      {/* Qt's drop overlay (SessionFileDrop): what a drop on this panel does. */}
      <Show when={drops.hint()}>{(text) => <div class="drop-overlay" aria-hidden="true"><span>{text()}</span></div>}</Show>
      <Show when={selected()}><Header /></Show>
        <div class="work" ref={measure}>
        <Show when={selected()} fallback={<div class="work-main"><EmptyDetail /></div>}>
          <div class="work-main">
            <div class="detail-tabs tabs" role="tablist">
              <For each={tabs().filter((t) => t.show)}>{(t) => (
                <button class="tab" role="tab" classList={{ on: detailTab() === t.id }} aria-selected={detailTab() === t.id} onClick={() => { if (t.id === "terminal") terminal.opened(); setDetailTab(t.id); }}>{t.label}</button>
              )}</For>
              <span class="grow" />
              <Show when={!inspectorOpen()}><button class="icon-btn small" aria-label="Show side panel" data-tip={inspectorHint("Show tasks, plan, agents, details and worktrees")} data-tip-pos="left" onClick={() => setInspectorOpen(true)}><Icon name="panel-right" size={15} /></button></Show>
            </div>
            <div class="tab-body">
              <Switch>
                <Match when={detailTab() === "activity"}>
                  <div class="activity-wrap" classList={{ "has-goal": !!goal().objective }} style={columnStyle()}>
                    <Show when={goal().objective}><button class="goal-pill" onClick={() => openInspector("plan")}><Icon name="circle-dot" size={13} /><b>Pursuing goal</b><span class="ell">{goal().objective}</span><span class="grow" /><span>Details</span><Icon name="chevron-right" size={12} /></button></Show>
                    <ActivityView source={activitySourceFor(selected()!.key)} />
                    {/* Only the subagent view stays read-only; archives keep their editable draft (Qt 3454). */}
                    <Show when={!subagentOpen()}>
                      <QueuePanel /><div class="cmp-col"><QuestionCard /><Composer /></div>
                    </Show>
                  </div>
                </Match>
                <Match when={detailTab() === "taskview"}><TaskView /></Match>
                <Match when={detailTab() === "terminal"}><TerminalTab /></Match>
                <Match when={detailTab() === "processes"}><ProcessesTab /></Match>
              </Switch>
            </div>
          </div>
        </Show>
        <Inspector available={available()} />
        </div>
        {/* The composer's questions and Clear session, also from the header on another tab; the field takes the focus back. */}
        <ComposerDialogs onClosed={() => { if (document.activeElement === document.body) document.querySelector<HTMLTextAreaElement>(".cmp-editor")?.focus(); }} />
    </section>
  );
}
