import { For, Show, Switch, Match, createEffect, createMemo, createSignal, onCleanup, on, type JSX } from "solid-js";
import { Icon } from "../lib/icons";
import { selected, setSubagentOpen, setDetailTab, setSelectedKey } from "../../store/ui";
import { inspectorOpen, inspectorTab, inspectorWidth, inspectorClosed, setInspectorOpen, setInspectorTab, setInspectorWidth, toggleInspectorCard, panelGeometry, inspectorHint, INSPECTOR_TABS, type InspectorTab } from "../../store/inspector";
import { followedFor, detailsFor, copyIdentity } from "../../store/actions";
import { computerOffline, orderedRows } from "../../store/catalog";
import { groupOf, grouping, INBOX } from "../../store/organization";
import { sessionIdentity, snapshotOf } from "./actions-ui";
import { TasksPanel, TaskTabBadge } from "../screens/Tasks";
import { object, text } from "../screens/tasks-ui";
import { sessionGoal, sessionPlan, usageRows } from "./inspector-data";
import { accountsPage } from "../../store/accounts";
import { sessionUsage, sessionUsageTarget } from "../../store/sessionUsage";
import { sessionUsageView } from "./session-usage-ui";
import * as bridge from "../../bridge";
import "../styles/inspector.css";

const LABEL: Record<InspectorTab, string> = { plan: "Plan & agents", details: "Details", worktrees: "Worktrees", tasks: "Tasks" };
const raw = () => selected() ? followedFor(selected()!.key).value : undefined;
function Card(props: { id: string; title: JSX.Element; aside?: JSX.Element; children: JSX.Element }) {
  const open = () => !inspectorClosed().has(props.id);
  return <section class="icard" classList={{ closed: !open() }}><header class="icard-head">
    <button class="icard-toggle" aria-expanded={open()} aria-controls={`icard-${props.id}`} onClick={() => toggleInspectorCard(props.id)}><span class="icard-title">{props.title}</span><Icon name="chevron-down" size={12} class="icard-chev" /></button>
    <Show when={props.aside}><span class="icard-aside">{props.aside}</span></Show>
  </header><Show when={open()}><div class="icard-body" id={`icard-${props.id}`}>{props.children}</div></Show></section>;
}
function Row(props: { icon: string; label?: string; mono?: boolean; title?: string; tail?: JSX.Element; children: JSX.Element }) {
  return <div class="irow" title={props.title}><span class="irow-ico"><Icon name={props.icon} size={16} /></span><span class="irow-val ell" classList={{ mono: props.mono }}>{props.children}</span>{props.tail}<Show when={props.label}><span class="irow-label">{props.label}</span></Show></div>;
}
function PlanAgents() {
  const s = selected, plan = createMemo(() => sessionPlan(raw())), goal = createMemo(() => sessionGoal(raw()));
  return <div class="insp-body">
    <Card id="plan-main" title="Main agent" aside={<span class="accent">{s()?.caption ?? "Not reported"}</span>}><Row icon="activity" label="status">{s()?.caption ?? "Not reported"}</Row><Row icon="terminal" label="action" mono title={s()?.summary}>{s()?.summary || "Not reported"}</Row></Card>
    <Show when={goal().objective}><Card id="plan-goal" title="Goal" aside={<span class="muted">{goal().status === "active" ? "Pursuing goal" : goal().status}</span>}><p class="icard-text">{goal().objective}</p><Show when={goal().completion}><div class="hint">Reported by the agent. Completion: {goal().completion}</div></Show></Card></Show>
    <Show when={plan().length}><Card id="plan-tasks" title="Tasks" aside={<span class="dim">{plan().filter((p) => p.status === "completed").length} of {plan().length} completed (last recorded)</span>}><For each={plan()}>{(p) => <Row icon={p.status === "completed" ? "check" : "circle-dot"} label={p.status === "completed" ? "Done" : p.status === "in_progress" ? "In progress" : "To do"}>{p.title}</Row>}</For></Card></Show>
    <Card id="plan-agents" title="Subagents" aside={<span class="dim">{s()?.subagents?.filter((c) => c.state === "working").length ?? 0} active / {s()?.subagents?.length ?? 0} total</span>}>
      <For each={s()?.subagents?.slice(0, 100)} fallback={<div class="hint">No subagent activity has been reported yet.</div>}>{(child) => <Row icon="circle" label={child.state === "working" ? "Working" : child.state === "ready" ? "Ready" : "Needs input"} title={child.summary}><button class="link" onClick={() => { setSubagentOpen(child.id); setDetailTab("activity"); }}>{child.name} →</button><span class="dim"> {child.summary}</span></Row>}</For>
    </Card>
  </div>;
}
function Details() {
  const s = () => selected()!;
  const details = () => detailsFor(s(), !computerOffline(s().computer), followedFor(s().key));
  const identity = () => sessionIdentity(s(), details());
  const value = (v: string | undefined) => v || "Not reported";
  const rows = createMemo(() => usageRows(raw()));
  const usage = () => sessionUsageView(object(raw()).session_usage);
  const account = createMemo(() => {
    const target = sessionUsageTarget(), row = s();
    const id = text(object(raw()).account_id) || (target?.host === row.computer && target.name === row.name && target.run_id === row.runId ? sessionUsage().id : "");
    return accountsPage().accounts.flatMap((a) => a.members).find((p) => p.host === row.computer && p.id === id);
  });
  return <div class="insp-body"><Card id="details-id" title={identity().title}><Row icon="key-round" mono title={identity().title} tail={<button class="icon-btn small" aria-label={identity().caption} title={identity().caption} disabled={!identity().value} onClick={() => void copyIdentity(snapshotOf(s()), details())}><Icon name="copy" size={13} /></button>}><span class="selectable">{value(identity().value)}</span></Row></Card>
    <Card id="details-session" title="Session"><Row icon="message-square" label="name" mono title={s().name}>{s().name}</Row><Row icon="monitor" label="machine">{s().machine}</Row><Row icon="folder" label="directory" mono title={s().cwdPath}>{value(s().cwdPath)}</Row><Row icon="git-branch" label="branch">{value(s().branch)}</Row><Row icon="layers" label="project">{grouping().headers.find((p) => p.id === groupOf(s().key))?.name ?? "Not reported"}</Row><Row icon="clock" label="last event">{s().updatedAt ? new Date(s().updatedAt! * 1000).toLocaleString() : "Not reported"}</Row><div class="hint">Activity stays on {s().machine}. Closing this window does not stop your agent.</div></Card>
    <Card id="details-account" title="Account"><Row icon="user-round-cog" label="account">{value(account()?.label || text(object(raw()).account_id))}</Row><Row icon="hard-drive" label="agent home" mono>{value(account()?.home || text(object(raw()).agent_home))}</Row><Row icon="sparkles" label="model">{value(s().model)} {s().effort}</Row></Card>
    <Card id="details-usage" title="Session usage"><Show when={rows().length} fallback={<div class="hint">No native usage reported for this conversation yet.</div>}><table class="usage"><tbody><For each={rows()}>{([label, val]) => <tr><td>{label}</td><td class="tabular">{val}</td></tr>}</For></tbody></table><Show when={usage().costNote}><div class="hint">{usage().costNote}</div></Show><div class="hint">{usage().footer}</div></Show></Card>
  </div>;
}
function Worktrees() {
  const [catalog, setCatalog] = createSignal<Record<string, unknown>>({}), [reading, setReading] = createSignal(false);
  const context = createMemo(() => { const s = selected(); return s?.cwdPath ? { computer: s.computer, path: s.cwdPath, key: `${s.computer}\n${s.cwdPath}` } : undefined; });
  let wanted = 0, busy = false, again = false, disposed = false;
  const read = async (refresh = false) => {
    if (busy) { again = true; return; }
    const target = context(); if (!target) return;
    busy = true; setReading(true); const ticket = ++wanted;
    try { const value = await bridge.worktreeCatalog(target.computer, target.path, refresh); if (!disposed && ticket === wanted && target.key === context()?.key) setCatalog(value); }
    catch { if (!disposed && target.key === context()?.key) setCatalog({ state: "unavailable" }); }
    finally { busy = false; if (!disposed) { setReading(false); if (again) { again = false; void read(); } } }
  };
  createEffect(() => { context(); ++wanted; setCatalog({}); void read(); });
  onCleanup(() => { disposed = true; ++wanted; });
  const trees = () => Array.isArray(catalog().worktrees) ? (catalog().worktrees as unknown[]).slice(0, 128).map(object) : [];
  return <div class="insp-body"><div class="insp-h"><b>Folder and worktrees</b><span class="grow" /><button class="icon-btn small" aria-label="Refresh" title="Refresh" disabled={reading() || !context()} onClick={() => void read(true)}><Icon name="refresh-cw" size={13} /></button></div>
    <div class="hint">{reading() ? "Reading folder…" : catalog().state === "ok" ? "Folder and worktrees on this computer" : catalog().state === "not_repo" ? "This folder is not a Git repository." : "Worktrees are unavailable for this session."}</div>
    <For each={trees()}>{(tree) => <Card id={`wt-${text(tree.path)}`} title={tree.kind === "main" ? "Main checkout" : "Linked worktree"} aside={<span class="dim">{text(tree.branch)}{tree.available === false ? " (unavailable)" : ""}</span>}><Row icon="folder" label="folder" mono title={text(tree.path)}>{text(tree.path)}</Row><For each={orderedRows().filter((s) => s.computer === selected()?.computer && s.cwdPath === tree.path).slice(0, 50)}>{(s) => <Row icon="corner-down-right" label={s.caption}><button class="link" onClick={() => { setSelectedKey(s.key); setDetailTab("activity"); }}>{s.title}</button></Row>}</For></Card>}</For>
  </div>;
}
export function Inspector(props: { available: number }) {
  const geometry = () => panelGeometry(props.available, inspectorWidth());
  let panel!: HTMLElement;
  let returnFocus: HTMLElement | undefined;
  const restoreFocus = () => queueMicrotask(() => { if (returnFocus?.isConnected && returnFocus !== document.body) returnFocus.focus(); else document.querySelector<HTMLButtonElement>("[aria-label='Show side panel']")?.focus(); });
  createEffect(on(inspectorOpen, (open, previous) => {
    if (open) returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    else if (previous) restoreFocus();
  }));
  const close = () => setInspectorOpen(false);
  createEffect(() => {
    if (!geometry().floating || !inspectorOpen()) return;
    const escape = (e: KeyboardEvent) => { if (e.key === "Escape" && !document.querySelector("dialog[open], [data-shortcut-context='none']")) { e.preventDefault(); close(); } };
    window.addEventListener("keydown", escape); onCleanup(() => window.removeEventListener("keydown", escape));
  });
  const resize = (e: PointerEvent) => {
    if (e.button !== 0) return; const el = e.currentTarget as HTMLElement; el.setPointerCapture(e.pointerId);
    const x = e.clientX, start = geometry().width;
    const move = (v: PointerEvent) => setInspectorWidth(start + x - v.clientX);
    const end = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", end); el.removeEventListener("pointercancel", end); };
    el.addEventListener("pointermove", move); el.addEventListener("pointerup", end); el.addEventListener("pointercancel", end);
  };
  return <Show when={inspectorOpen()}><aside ref={panel} class="inspector" classList={{ floating: geometry().floating }} style={{ width: `${geometry().width}px` }} aria-label="Side panel">
    <div class="insp-resize" tabindex="0" role="separator" aria-orientation="vertical" aria-label="Side panel width" aria-valuemin={240} aria-valuemax={900} aria-valuenow={inspectorWidth()} onPointerDown={resize} onDblClick={() => setInspectorWidth(400)} onKeyDown={(e) => { if (["ArrowLeft", "ArrowRight", "Home"].includes(e.key)) { e.preventDefault(); setInspectorWidth(e.key === "Home" ? 400 : inspectorWidth() + (e.key === "ArrowLeft" ? 20 : -20)); } }}><i /></div>
    <div class="insp-tabs tabs" role="tablist" aria-label="Side panel tabs"><For each={INSPECTOR_TABS}>{(tab) => <button class="tab" role="tab" classList={{ on: inspectorTab() === tab }} aria-selected={inspectorTab() === tab} tabindex={inspectorTab() === tab ? 0 : -1} onClick={() => setInspectorTab(tab)} onKeyDown={(e) => { const i = INSPECTOR_TABS.indexOf(tab), next = e.key === "ArrowRight" ? (i + 1) % 4 : e.key === "ArrowLeft" ? (i + 3) % 4 : e.key === "Home" ? 0 : e.key === "End" ? 3 : -1; if (next >= 0) { e.preventDefault(); setInspectorTab(INSPECTOR_TABS[next]); panel.querySelectorAll<HTMLButtonElement>(".insp-tabs .tab")[next]?.focus(); } }}>{LABEL[tab]}{tab === "tasks" ? <TaskTabBadge /> : null}</button>}</For><span class="grow" /><button class="icon-btn small" aria-label="Collapse side panel" data-tip={inspectorHint("Collapse side panel")} data-tip-pos="left" onClick={close}><Icon name="panel-right-close" size={14} /></button></div>
    <div class="insp-scroll" classList={{ tasks: inspectorTab() === "tasks" }}><Switch><Match when={inspectorTab() === "plan"}><PlanAgents /></Match><Match when={inspectorTab() === "details" && selected()}><Details /></Match><Match when={inspectorTab() === "worktrees"}><Worktrees /></Match><Match when={inspectorTab() === "tasks"}><TasksPanel /></Match></Switch></div>
  </aside></Show>;
}
