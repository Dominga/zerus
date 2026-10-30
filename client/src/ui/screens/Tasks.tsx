import { createTaskReason } from "./TaskReason";
import { canCancel, requestTaskCancellation, PAUSE_REASON } from "../../store/taskCancel";
import { For, Show, Suspense, createEffect, createMemo, createSignal, lazy, on, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { statusOf } from "../sessions/SessionCard";
import { selectedTask, openTask, requestTaskRename } from "../../store/taskNavigation";
import { tasks } from "../../store/tasks";
import { flag, setSetting } from "../../store/settings";
import { now, selected, setMenu } from "../../store/ui";
import { STAGES, STAGE_ICON, STAGE_LABEL, TASK_PAGE, assignmentLabel, boardRows, belongsTo, executorRef, taskPageOffset, type Stage, type Task } from "./tasks-ui";
import { assign, assigningTask, canAssign, draggedTask, setDraggedTask, taskDragStart, readTaskDrag, TASK_MIME } from "../../store/taskAssignment";
import { ACTION_ICON, ACTION_LABEL, actOnTask, boardActions, canDrag, primaryAction, reorderGap, stageDropAction, writingTask } from "../../store/taskBoard";
import { taskProject as project, projectSessions as sessions, findTaskSession as find, projectTasks as rows } from "../../store/taskContext";
import "../styles/tasks.css";

const TaskCreate = lazy(() => import("./TaskCreate"));

export function TaskTabBadge() { return <Show when={project() && tasks.loaded()}><span class="tk-tabbadge"><Show when={rows().filter((t) => t.stage === "review").length}>{(n) => <span class="tk-tb review">{n()}</span>}</Show><Show when={rows().filter((t) => t.stage === "todo").length}>{(n) => <span class="tk-tb todo">{n()}</span>}</Show></span></Show>; }
const FILTERS = ["current", ...STAGES] as const;
type Filter = typeof FILTERS[number];
const LABEL = { current: "Active", ...STAGE_LABEL }, ICON = { current: "activity", ...STAGE_ICON };
const EMPTY = { current: "No tasks in progress or review", review: "Nothing to review", active: "No session is running a task", todo: "No tasks in To do", backlog: "Empty", done: "Nothing yet" };
export function TasksPanel() {
  const [stage, setStage] = createSignal<Filter>("current"), [older, setOlder] = createSignal(false), [at, setAt] = createSignal(0);
  const [assignment, setAssignment] = createSignal<Task>(), [query, setQuery] = createSignal("");
  const { show: showReason, clear: clearReason, Tip: ReasonTip } = createTaskReason();
  createEffect(on(() => project()?.id, clearReason));
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
  const drag = () => draggedTask()?.project === project()?.id ? rows().find((t) => t.uuid === draggedTask()?.uuid) : undefined;
  const [dropTile, setDropTile] = createSignal<Filter>();
  let scroller!: HTMLDivElement, cards!: HTMLDivElement, timer: number | undefined, step = 0;
  const [lineAt, setLineAt] = createSignal<number>();
  const endDrag = () => { setLineAt(undefined); clearInterval(timer); timer = undefined; step = 0; };
  onCleanup(endDrag);
  createEffect(on(() => [project()?.id, stage(), at(), draggedTask()] as const, endDrag));
  const canReorder = () => { const t = drag(); return !!t && canDrag(t) && !writingTask() && (t.stage === stage() || stage() === "current" && ["active", "review"].includes(t.stage)); };
  const gapAt = (y: number) => {
    const t = drag(), elements = t ? [...cards.querySelectorAll<HTMLElement>(".tk-card")].filter((r) => r.dataset.stage === t.stage) : [];
    const gap = t && reorderGap(elements.map((r) => { const b = r.getBoundingClientRect(); return { uuid: r.dataset.task!, top: b.top, height: b.height }; }), t.uuid, y);
    return { elements, gap };
  };
  const over = (e: DragEvent) => {
    if (!e.dataTransfer?.types.includes(TASK_MIME) || !canReorder()) return endDrag();
    e.preventDefault(); e.stopPropagation(); e.dataTransfer.dropEffect = "move";
    const { elements, gap } = gapAt(e.clientY), edge = gap && elements[gap.before ? gap.gap : elements.length - 1];
    setLineAt(edge && gap ? edge.offsetTop + (gap.before ? -3 : edge.offsetHeight + 1) : undefined);
    const r = scroller.getBoundingClientRect(); step = e.clientY < r.top + 28 ? -8 : e.clientY > r.bottom - 28 ? 8 : 0;
    if (!step) { clearInterval(timer); timer = undefined; }
    else if (!timer) timer = window.setInterval(() => { scroller.scrollTop += step; }, 30);
  };
  const dropped = (e: DragEvent) => {
    if (!canReorder()) return endDrag();
    e.preventDefault(); e.stopPropagation();
    const task = readTaskDrag(e, project()!.id), { gap } = gapAt(e.clientY);
    endDrag(); setDraggedTask(undefined);
    if (task && gap) void actOnTask(project()!.id, task, { kind: "reorder", anchor: gap.anchor, before: gap.before });
  };
  const tileOver = (e: DragEvent, filter: Filter) => {
    if (!e.dataTransfer?.types.includes(TASK_MIME) || writingTask() || !stageDropAction(drag(), filter)) return false;
    e.preventDefault(); e.stopPropagation(); e.dataTransfer.dropEffect = "move"; endDrag(); return true;
  };
  const menu = (e: MouseEvent | KeyboardEvent, t: Task) => { e.preventDefault(); e.stopPropagation(); const anchor = e.currentTarget as HTMLElement, el = anchor.getBoundingClientRect(), projectId = project()!.id; setMenu({ x: el.left, y: el.bottom, items: [
    { label: "Task details", icon: "file-text", action: () => openTask(projectId, t.uuid) },
    { label: "Rename…", icon: "pencil", disabled: !t.writable || !!writingTask(), action: () => requestTaskRename(projectId, t.uuid) },
    ...boardActions(t.stage).map((action) => ({ label: ACTION_LABEL[action], icon: ACTION_ICON[action], disabled: !t.writable || !!writingTask(), action: () => void actOnTask(projectId, t, { kind: action }) })),
    ...(t.stage === "active" ? [{ label: "Cancel task…", icon: "x", danger: true, disabled: !canCancel(t) || !!writingTask(), action: () => requestTaskCancellation(projectId, t, anchor) }] : []),
    { label: "Assign to session…", icon: "user-round-cog", disabled: !canAssign(t, projectId) || !!assigningTask(), action: () => { if (project()?.id !== projectId) return; setQuery(""); setAssignment(t); } },
  ] }); };
  let picker!: HTMLDivElement;
  createEffect(() => { if (!assignment()) return; const close = (e: PointerEvent) => { if (!picker?.contains(e.target as Node)) setAssignment(undefined); }; document.addEventListener("pointerdown", close); onCleanup(() => document.removeEventListener("pointerdown", close)); queueMicrotask(() => picker?.querySelector<HTMLInputElement>("input")?.focus()); });
  const choices = () => sessions().filter((s) => canAssign(assignment()!, project()!.id, s) && `${s.title} ${s.name} ${s.machine}`.toLowerCase().includes(query().toLowerCase()));
  return <div class="tk-panel"><Suspense><TaskCreate project={project()?.id} scope={project() ? `${visible().length} ${visible().length === 1 ? "task" : "tasks"}` : "Open a project session to see tasks"} accessible={!!project()?.accessible} backlog={stage() === "backlog"}>
    <button class="tk-sq" classList={{ on: flag("tasks/onlySession") }} aria-label="Only this session" title="Only this session" aria-pressed={flag("tasks/onlySession")} disabled={!project()} onClick={() => void setSetting("tasks/onlySession", !flag("tasks/onlySession"))}><Icon name="filter" size={15} /></button>
    <button class="tk-sq" aria-label="Task settings" title="Task settings" disabled><Icon name="settings" size={15} /></button>
  </TaskCreate></Suspense>
    <Show when={project()} fallback={<div class="tk-empty tk-noproject"><Icon name="list-todo" size={22} /><span>{selected() && selected()!.state !== "archived" ? "Tasks belong to a project. Move this session into one to keep tasks for it." : "Open a project session to see tasks"}</span></div>}>
      <div class="tk-tiles" role="tablist" aria-label="Task stages"><For each={FILTERS}>{(s) => <button class={`tk-tile st-${s}`} classList={{ on: stage() === s, drop: dropTile() === s }} onDragOver={(e) => { if (tileOver(e, s)) setDropTile(s); }} onDragLeave={() => setDropTile(undefined)} onDrop={(e) => { setDropTile(undefined); if (!tileOver(e, s)) return; const task = readTaskDrag(e, project()!.id), action = stageDropAction(task, s); setDraggedTask(undefined); if (task && action) void actOnTask(project()!.id, task, action); }} role="tab" aria-label={`${LABEL[s]} ${stageRows(s).length}`} aria-selected={stage() === s} tabindex={stage() === s ? 0 : -1} onClick={() => setStage(s)} onKeyDown={(e) => { const i = FILTERS.indexOf(s), next = e.key === "ArrowRight" ? (i + 1) % 6 : e.key === "ArrowLeft" ? (i + 5) % 6 : e.key === "Home" ? 0 : e.key === "End" ? 5 : -1; if (next >= 0) { e.preventDefault(); setStage(FILTERS[next]); e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>("button")[next]?.focus(); } }}><span class="tk-tile-ico"><Icon name={ICON[s]} size={14} /></span><span class="tk-tile-label">{LABEL[s]}</span><b class="tk-tile-count">{stageRows(s).length}</b></button>}</For></div>
      <Show when={tasks.error()}><div class="notice danger tk-line" role="alert">Tasks could not be read: {tasks.error()}</div></Show>
      <Show when={tasks.damaged()}><div class="notice warn tk-line">{tasks.damaged()} task files of {project()!.name} could not be read.</div></Show>
      <Show when={tasks.truncated()}><div class="notice warn tk-line">Showing the first 10,000 tasks.</div></Show>
      <Show when={drag() && canAssign(drag()!, project()!.id)}><div class="tk-drophint" role="status"><Icon name="corner-down-right" size={13} />Drop onto a session in the list to assign it; onto the project header for Any session.</div></Show>
      <div class="tk-scroll" ref={scroller} onDragOver={over} onDrop={dropped} onDragLeave={(e) => { if (!scroller.contains(e.relatedTarget as Node)) endDrag(); }}><div class="tk-cards" ref={cards} role="list" aria-label={`${LABEL[stage()]} tasks`}><For each={shown()}>{(t) => <article class={`tk-card st-${t.stage}`} data-task={t.uuid} data-stage={t.stage} classList={{ sel: selectedTask()?.uuid === t.uuid, mine: !!selected() && belongsTo(t, selected()!), quiet: ["backlog", "done"].includes(t.stage), dragging: draggedTask()?.uuid === t.uuid }} tabindex="0" role="listitem" aria-label={`${t.id}: ${t.title}`} draggable={canDrag(t) && !writingTask()} onDragStart={(e) => taskDragStart(e, t)} onDragEnd={() => { setDraggedTask(undefined); setDropTile(undefined); endDrag(); }} onClick={() => openTask(project()!.id, t.uuid)} onContextMenu={(e) => menu(e, t)} onKeyDown={(e) => { if (e.target !== e.currentTarget) return; if (e.key === "Enter" || e.key === " ") { e.preventDefault(); openTask(project()!.id, t.uuid); } else if (e.key === "ContextMenu" || e.shiftKey && e.key === "F10") menu(e, t); }}>
        <span class={`tk-glyph st-${t.stage}`} aria-hidden="true"><Show when={t.stage === "todo"}>{t.queue}</Show><Show when={["review", "done"].includes(t.stage)}><Icon name="check" size={11} /></Show></span>
        <div class="tk-card-main"><div class="tk-card-title">{t.title || "Untitled task"}</div><div class="tk-meta"><Show when={stage() === "current"}><span class="tk-meta-stage">{STAGE_LABEL[t.stage as Stage]}</span></Show><span class="mono tk-id">{t.id}</span><span class="tk-target ell" title={assignmentLabel(t)}>{t.worker ? `Subagent: ${t.worker}` : assignmentLabel(t)}</span><Show when={t.stage === "active" && find()(executorRef(t))}>{(s) => <span class={`hbadge t-${statusOf(s())}`}>{s().caption}</span>}</Show></div><Show when={t.stage === "review" && (t.summary || t.cancelReason)}><p class="tk-summary">{t.cancelReason || t.summary}</p></Show></div>
        <div class="tk-card-acts"><Show when={t.stage === "active"}><button class="icon-btn tk-act tk-primary blocked" aria-label="Pause task" aria-description={PAUSE_REASON} title={PAUSE_REASON} onClick={(e) => { e.stopPropagation(); openTask(project()!.id, t.uuid); showReason(e.currentTarget, PAUSE_REASON); }}><Icon name="pause" size={15} /></button></Show><Show when={primaryAction(t.stage)}>{(action) => <button class="icon-btn tk-act tk-primary" aria-label={ACTION_LABEL[action()]} title={ACTION_LABEL[action()]} disabled={!t.writable || !!writingTask()} onClick={(e) => { e.stopPropagation(); void actOnTask(project()!.id, t, { kind: action() }); }}><Icon name={ACTION_ICON[action()]} size={15} /></button>}</Show><button class="icon-btn tk-act" aria-label="Task actions" title="Task actions" aria-haspopup="menu" onClick={(e) => menu(e, t)}><Icon name="ellipsis" size={15} /></button></div>
      </article>}</For><div class="tk-dropline" classList={{ on: lineAt() !== undefined }} style={{ translate: `0 ${lineAt() ?? 0}px` }} aria-hidden="true" /></div><Show when={!visible().length && !tasks.error()}><div class="tk-empty"><Icon name={ICON[stage()]} size={20} /><span>{!project()!.accessible ? "This project is unavailable on this computer." : tasks.loaded() ? EMPTY[stage()] : "Loading tasks…"}</span></div></Show>
        <Show when={stage() === "done" && stageRows("done", true).length}><button class="tk-older" onClick={() => setOlder(!older())}>{older() ? "Hide older" : `Show older (${stageRows("done", true).length})`}</button></Show>
      </div>
      <Show when={visible().length > TASK_PAGE}><div class="tasks-navigation"><span class="muted">{at() + 1}–{Math.min(at() + TASK_PAGE, visible().length)} of {visible().length}</span><span class="grow" /><button class="btn small" disabled={!at()} onClick={() => setAt(Math.max(0, at() - TASK_PAGE))}>Previous</button><button class="btn small" disabled={at() + TASK_PAGE >= visible().length} onClick={() => setAt(at() + TASK_PAGE)}>Next</button></div></Show>
    </Show>
    <Show when={assignment()}>{(task) => <div ref={picker} class="tk-assign menu" data-shortcut-context="none" role="dialog" aria-label="Assign to session" onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setAssignment(undefined); } }}><b>Assign to session…</b><input class="field tk-assign-search" placeholder="Search sessions…" aria-label="Search sessions" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} /><div class="tk-assign-list"><button class="menu-item" onClick={() => { void assign(project()!.id, task()); setAssignment(undefined); }}>Any session</button><For each={choices().slice(0, 50)}>{(s) => <button class="menu-item" onClick={() => { void assign(project()!.id, task(), { ...s }); setAssignment(undefined); }}>{s.title} <span class="muted">@{s.machine}</span></button>}</For><Show when={choices().length > 50}><div class="hint">Refine the search to see more sessions.</div></Show></div></div>}</Show>
    <ReasonTip />
  </div>;
}
