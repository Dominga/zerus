import { For, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { TaskInspector } from "./TaskInspector";
import { statusOf } from "../sessions/SessionCard";
import { orderedRows } from "../../store/catalog";
import { groupOf, grouping, INBOX } from "../../store/organization";
import { selectedTask, openTask } from "../../store/taskNavigation";
import { tasks } from "../../store/tasks";
import { flag, setSetting } from "../../store/settings";
import { pageHidden } from "../../store/visibility";
import { now, selected, setSelectedKey, setDetailTab, setMenu } from "../../store/ui";
import { STAGES, STAGE_ICON, STAGE_LABEL, TASK_PAGE, assignmentLabel, boardRows, belongsTo, executorRef, projectTask, taskPageOffset, taskSessions, type Stage, type Task } from "./tasks-ui";
import { assign, assigningTask, canAssign, draggedTask, setDraggedTask, taskDragStart } from "../../store/taskAssignment";
import "../styles/tasks.css";

const project = () => { const s = selected(); return s && s.state !== "archived" ? grouping().headers.find((p) => p.id !== INBOX && p.id === groupOf(s.key)) : undefined; };
const sessions = createMemo(() => orderedRows().filter((s) => s.state !== "archived" && groupOf(s.key) === project()?.id));
const find = createMemo(() => taskSessions(sessions()));
const rows = createMemo(() => tasks.project() === project()?.id ? tasks.tasks().map((t) => projectTask(t, find())) : []);
export function followProjectTasks() {
  createEffect(() => tasks.follow(!pageHidden() && project()?.accessible ? project()!.id : undefined));
  onCleanup(() => tasks.follow(undefined));
}
export function TaskTabBadge() { return <Show when={project() && tasks.loaded()}><span class="tk-tabbadge"><Show when={rows().filter((t) => t.stage === "review").length}>{(n) => <span class="tk-tb review">{n()}</span>}</Show><Show when={rows().filter((t) => t.stage === "todo").length}>{(n) => <span class="tk-tb todo">{n()}</span>}</Show></span></Show>; }
export function TaskView() {
  const chosen = () => selectedTask()?.project === project()?.id ? rows().find((t) => t.uuid === selectedTask()?.uuid) : undefined;
  return <div class="tv"><Show when={chosen()} fallback={<p class="tasks-empty">Select a task in the Tasks panel to see its details.<br />Open the side panel's Tasks tab and click a card.</p>}>{(task) => <TaskInspector project={project()!.id} task={tasks.tasks().find((t) => t.uuid === task().uuid)!} presentation={task()} executorAvailable={!!find()(executorRef(task()))} openExecutor={() => { const s = find()(executorRef(task())); if (s) { setSelectedKey(s.key); setDetailTab("activity"); } }} />}</Show></div>;
}
const FILTERS = ["current", ...STAGES] as const;
type Filter = typeof FILTERS[number];
const LABEL = { current: "Active", ...STAGE_LABEL }, ICON = { current: "activity", ...STAGE_ICON };
const EMPTY = { current: "No tasks in progress or review", review: "Nothing to review", active: "No session is running a task", todo: "No tasks in To do", backlog: "Empty", done: "Nothing yet" };
export function TasksPanel() {
  const [stage, setStage] = createSignal<Filter>("current"), [older, setOlder] = createSignal(false), [at, setAt] = createSignal(0);
  const [assignment, setAssignment] = createSignal<Task>(), [query, setQuery] = createSignal("");
  const only = () => flag("tasks/onlySession") ? selected() : undefined;
  const ageNow = createMemo(() => Math.floor(now() / 60) * 60_000);
  const stageRows = (filter: Filter, old = false) => filter === "current" ? rows().filter((t) => ["review", "active"].includes(t.stage) && (!only() || belongsTo(t, only()!))) : boardRows(rows(), filter, only(), ageNow(), old);
  const visible = createMemo(() => tasks.error() ? [] : [...stageRows(stage()), ...(stage() === "done" && older() ? stageRows("done", true) : [])]);
  const shown = () => visible().slice(at(), at() + TASK_PAGE);
  createEffect(on(() => project()?.id, () => { setAt(0); setOlder(false); setStage("current"); setAssignment(undefined); }));
  createEffect(on(() => [stage(), only()?.key, older()] as const, () => setAt(0)));
  createEffect(on(visible, (next, previous) => { if (previous) setAt(taskPageOffset(previous, next, at())); }));
  createEffect(() => { if (at() >= visible().length) setAt(Math.max(0, Math.floor((visible().length - 1) / TASK_PAGE) * TASK_PAGE)); });
  let revealed = "";
  createEffect(() => {
    const id = selectedTask(), task = id && id.project === project()?.id ? rows().find((t) => t.uuid === id.uuid) : undefined;
    const key = id ? `${id.project}\n${id.uuid}` : "";
    if (!task || key === revealed) return;
    revealed = key;
    if (!(stage() === "current" && ["review", "active"].includes(task.stage))) setStage(task.stage as Stage);
    if (only() && !belongsTo(task, only()!)) void setSetting("tasks/onlySession", false);
    if (task.stage === "done" && stageRows("done", true).some((t) => t.uuid === task.uuid)) setOlder(true);
  });
  const menu = (e: MouseEvent | KeyboardEvent, t: Task) => { e.preventDefault(); e.stopPropagation(); const el = (e.currentTarget as HTMLElement).getBoundingClientRect(); setMenu({ x: el.left, y: el.bottom, items: [
    { label: "Open task", icon: "external-link", action: () => openTask(project()!.id, t.uuid) },
    { label: "Assign to session…", icon: "user-round-cog", disabled: !canAssign(t, project()!.id) || !!assigningTask(), action: () => { setQuery(""); setAssignment(t); } },
  ] }); };
  let picker!: HTMLDivElement;
  createEffect(() => { if (!assignment()) return; const close = (e: PointerEvent) => { if (!picker?.contains(e.target as Node)) setAssignment(undefined); }; document.addEventListener("pointerdown", close); onCleanup(() => document.removeEventListener("pointerdown", close)); queueMicrotask(() => picker?.querySelector<HTMLInputElement>("input")?.focus()); });
  const choices = () => sessions().filter((s) => canAssign(assignment()!, project()!.id, s) && `${s.title} ${s.name} ${s.machine}`.toLowerCase().includes(query().toLowerCase()));
  return <div class="tk-panel"><div class="tk-head"><span class="tk-scope">{project() ? `${rows().length} tasks` : "Tasks"}</span>
    <button class="tk-sq add" aria-label="New task" title="New task" disabled><Icon name="plus" size={16} /></button>
    <button class="tk-sq" classList={{ on: flag("tasks/onlySession") }} aria-label="Only this session" title="Only this session" aria-pressed={flag("tasks/onlySession")} disabled={!project()} onClick={() => void setSetting("tasks/onlySession", !flag("tasks/onlySession"))}><Icon name="filter" size={15} /></button>
    <button class="tk-sq" aria-label="Task settings" title="Task settings" disabled><Icon name="settings" size={15} /></button>
  </div>
    <Show when={project()} fallback={<div class="tk-empty tk-noproject"><Icon name="list-todo" size={22} /><span>{selected() && selected()!.state !== "archived" ? "Tasks belong to a project. Move this session into one to keep tasks for it." : "Open a project session to see tasks"}</span></div>}>
      <div class="tk-tiles" role="tablist" aria-label="Task stages"><For each={FILTERS}>{(s) => <button class={`tk-tile st-${s}`} classList={{ on: stage() === s }} role="tab" aria-label={`${LABEL[s]} ${stageRows(s).length}`} aria-selected={stage() === s} tabindex={stage() === s ? 0 : -1} onClick={() => setStage(s)} onKeyDown={(e) => { const i = FILTERS.indexOf(s), next = e.key === "ArrowRight" ? (i + 1) % 6 : e.key === "ArrowLeft" ? (i + 5) % 6 : e.key === "Home" ? 0 : e.key === "End" ? 5 : -1; if (next >= 0) { e.preventDefault(); setStage(FILTERS[next]); e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>("button")[next]?.focus(); } }}><span class="tk-tile-ico"><Icon name={ICON[s]} size={14} /></span><span class="tk-tile-label">{LABEL[s]}</span><b class="tk-tile-count">{stageRows(s).length}</b></button>}</For></div>
      <Show when={tasks.error()}><div class="notice danger tk-line" role="alert">Tasks could not be read: {tasks.error()}</div></Show>
      <Show when={tasks.damaged()}><div class="notice warn tk-line">{tasks.damaged()} task files of {project()!.name} could not be read.</div></Show>
      <Show when={tasks.truncated()}><div class="notice warn tk-line">Showing the first 10,000 tasks.</div></Show>
      <Show when={draggedTask()}><div class="tk-drophint" role="status"><Icon name="corner-down-right" size={13} />Drop onto a session in the list to assign it; onto the project header for Any session.</div></Show>
      <div class="tk-scroll"><div class="tk-cards" role="list" aria-label={`${LABEL[stage()]} tasks`}><For each={shown()}>{(t) => <article class={`tk-card st-${t.stage}`} classList={{ sel: selectedTask()?.uuid === t.uuid, mine: !!selected() && belongsTo(t, selected()!), quiet: ["backlog", "done"].includes(t.stage), dragging: draggedTask()?.uuid === t.uuid }} tabindex="0" role="listitem" aria-label={`${t.id}: ${t.title}`} draggable={canAssign(t, project()!.id)} onDragStart={(e) => taskDragStart(e, t)} onDragEnd={() => setDraggedTask(undefined)} onClick={() => openTask(project()!.id, t.uuid)} onContextMenu={(e) => menu(e, t)} onKeyDown={(e) => { if (e.target !== e.currentTarget) return; if (e.key === "Enter" || e.key === " ") { e.preventDefault(); openTask(project()!.id, t.uuid); } else if (e.key === "ContextMenu" || e.shiftKey && e.key === "F10") menu(e, t); }}>
        <span class={`tk-glyph st-${t.stage}`} aria-hidden="true"><Show when={t.stage === "todo"}>{t.queue}</Show><Show when={["review", "done"].includes(t.stage)}><Icon name="check" size={11} /></Show></span>
        <div class="tk-card-main"><div class="tk-card-title">{t.title || "Untitled task"}</div><div class="tk-meta"><Show when={stage() === "current"}><span class="tk-meta-stage">{STAGE_LABEL[t.stage as Stage]}</span></Show><span class="mono tk-id">{t.id}</span><span class="tk-target ell" title={assignmentLabel(t)}>{t.worker ? `Subagent: ${t.worker}` : assignmentLabel(t)}</span><Show when={t.stage === "active" && find()(executorRef(t))}>{(s) => <span class={`hbadge t-${statusOf(s())}`}>{s().caption}</span>}</Show></div><Show when={t.stage === "review" && (t.summary || t.cancelReason)}><p class="tk-summary">{t.cancelReason || t.summary}</p></Show></div>
        <div class="tk-card-acts"><button class="icon-btn tk-act" aria-label="Task actions" title="Task actions" aria-haspopup="menu" onClick={(e) => menu(e, t)}><Icon name="ellipsis" size={15} /></button></div>
      </article>}</For></div><Show when={!visible().length && !tasks.error()}><div class="tk-empty"><Icon name={ICON[stage()]} size={20} /><span>{!project()!.accessible ? "This project is unavailable on this computer." : tasks.loaded() ? EMPTY[stage()] : "Loading tasks…"}</span></div></Show>
        <Show when={stage() === "done" && stageRows("done", true).length}><button class="tk-older" onClick={() => setOlder(!older())}>{older() ? "Hide older" : `Show older (${stageRows("done", true).length})`}</button></Show>
      </div>
      <Show when={visible().length > TASK_PAGE}><div class="tasks-navigation"><span class="muted">{at() + 1}–{Math.min(at() + TASK_PAGE, visible().length)} of {visible().length}</span><span class="grow" /><button class="btn small" disabled={!at()} onClick={() => setAt(Math.max(0, at() - TASK_PAGE))}>Previous</button><button class="btn small" disabled={at() + TASK_PAGE >= visible().length} onClick={() => setAt(at() + TASK_PAGE)}>Next</button></div></Show>
    </Show>
    <Show when={assignment()}>{(task) => <div ref={picker} class="tk-assign menu" data-shortcut-context="none" role="dialog" aria-label="Assign to session" onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setAssignment(undefined); } }}><b>Assign to session…</b><input class="field tk-assign-search" placeholder="Search sessions…" aria-label="Search sessions" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} /><div class="tk-assign-list"><button class="menu-item" onClick={() => { void assign(project()!.id, task()); setAssignment(undefined); }}>Any session</button><For each={choices().slice(0, 50)}>{(s) => <button class="menu-item" onClick={() => { void assign(project()!.id, task(), { ...s }); setAssignment(undefined); }}>{s.title} <span class="muted">@{s.machine}</span></button>}</For><Show when={choices().length > 50}><div class="hint">Refine the search to see more sessions.</div></Show></div></div>}</Show>
  </div>;
}
