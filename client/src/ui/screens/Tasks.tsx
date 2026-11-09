import { supportsTaskStart, taskStartBlocker, startTask } from "../../store/taskStart";
import { taskSettings } from "../../store/taskSettings";
import { TaskStartButton } from "./TaskStartButton";
import { createTaskReason } from "./TaskReason";
import { canCancel, requestTaskCancellation, PAUSE_REASON } from "../../store/taskCancel";
import { For, Show, Suspense, createEffect, createMemo, createSignal, lazy, on, onCleanup, untrack } from "solid-js";
import { Icon } from "../lib/icons";
import { statusOf } from "../sessions/SessionCard";
import { selectedTask, openTask, requestTaskRename } from "../../store/taskNavigation";
import { tasks } from "../../store/tasks";
import { flag, setSetting } from "../../store/settings";
import { now, selected, setMenu } from "../../store/ui";
import { RECENT_DONE, STAGES, STAGE_ICON, STAGE_LABEL, stageLabel, TASK_PAGE, assignmentLabel, boardRows, boardTask, belongsTo, keepsSessionFilter, executorRef, sessionIdentity, taskPageOffset, type Stage, type Task } from "./tasks-ui";
import { openTaskAssignment, assigningTask, canAssign, draggedTask, setDraggedTask, taskDragStart, readTaskDrag, TASK_MIME, assign, taskAssignmentAt } from "../../store/taskAssignment";
import { POINTER_DRAG, pointerDrag } from "../lib/pointerDrag";
import { ACTION_ICON, ACTION_LABEL, actionLabel, actOnTask, boardActions, canDrag, primaryAction, reorderGap, stageDropAction, writingTask } from "../../store/taskBoard";
import { taskProject as project, projectSessions as sessions, findTaskSession as find, projectTasks as rows, sessionTaskRows } from "../../store/taskContext";
import "../styles/tasks.css";
import { tr, trNoop } from "../../i18n";
import { copyReviewOutcomes } from "../../store/taskCopyReview";

const TaskCreate = lazy(() => import("./TaskCreate"));
const TaskCopyReview = lazy(() => import("./TaskCopyReview"));

const FILTERS = ["current", ...STAGES] as const;
type Filter = typeof FILTERS[number];
const LABEL: Record<string, string> = { current: trNoop("Active"), ...STAGE_LABEL }, ICON = { current: "activity", ...STAGE_ICON };
const EMPTY = { current: tr("No tasks in progress or review"), review: tr("Nothing to review"), active: tr("No session is running a task"), todo: tr("No tasks in To do"), backlog: tr("Empty"), done: tr("Nothing yet") };
export function TasksPanel() {
  const [stage, setStage] = createSignal<Filter>("current"), [older, setOlder] = createSignal(false), [at, setAt] = createSignal(0);
  const { show: showReason, clear: clearReason, Tip: ReasonTip } = createTaskReason();
  createEffect(on(() => project()?.id, clearReason));
  const only = () => flag("tasks/onlySession") ? selected() : undefined;
  const ageNow = createMemo(() => Math.floor(now() / 60) * 60_000);
  const stagesOf = (filter: Filter): Stage[] => filter === "current" ? ["review", "active"] : [filter];
  // Only this session over a windowed list: hgs pages the session's own rows by its identity.
  createEffect(() => {
    const s = only(), id = s ? sessionIdentity(s) : "", owner = tasks.windowed() && tasks.project() === project()?.id ? tasks.project() : undefined;
    const scope = s && id ? { host: s.computer, sessionId: id } : undefined, stages = stagesOf(stage());
    untrack(() => tasks.session.show(owner, scope, stages));
  });
  // A closed panel reads no more pages of the view.
  onCleanup(() => tasks.session.suspend());
  const view = tasks.session;
  const stageRows = (filter: Filter, old = false) => {
    const own = sessionTaskRows();
    if (own) return filter === "current" ? [...own.review, ...own.active] : own[filter].filter((t) => filter !== "done" || (ageNow() - t.stageAt > RECENT_DONE) === old);
    return filter === "current" ? rows().filter((t) => ["review", "active"].includes(t.stage) && (!only() || belongsTo(t, only()!))) : boardRows(rows(), filter, only(), ageNow(), old);
  };
  const visible = createMemo(() => tasks.error() ? [] : [...stageRows(stage()), ...(stage() === "done" && older() ? stageRows("done", true) : [])]);
  const shown = () => visible().slice(at(), at() + TASK_PAGE);
  // A windowed list knows each stage's total, and hgs counts a session's view;
  // a session filter over a whole list counts the rows it shows.
  const tileCount = (s: Filter) => sessionTaskRows() ? stagesOf(s).reduce((sum, st) => sum + view.total(st), 0)
    : !tasks.windowed() || only() ? stageRows(s).length
    : s === "current" ? tasks.total("review") + tasks.total("active") : tasks.total(s);
  const remaining = (s: Stage) => sessionTaskRows() ? view.remaining(s) : tasks.remaining(s);
  const reading = (s: Stage) => sessionTaskRows() ? view.loading()[s] : tasks.loading()[s];
  const pageError = () => sessionTaskRows() ? view.error() : tasks.pageError();
  const listed = () => sessionTaskRows() ? stagesOf(stage()).every((s) => view.read(s)) : tasks.loaded();
  const moreStages = () => stagesOf(stage()).filter((s) => remaining(s) > 0);
  const loadMore = async (s: Stage) => {
    const before = visible().length;
    await (sessionTaskRows() ? view.more(s) : tasks.more(s));
    if (visible().length > before) setAt(Math.floor(before / TASK_PAGE) * TASK_PAGE);
  };
  createEffect(on(() => project()?.id, () => { setAt(0); setOlder(false); setStage("current"); }));
  createEffect(on(() => [stage(), only()?.key, older()] as const, () => setAt(0)));
  createEffect(on(visible, (next, previous) => { if (previous) setAt(taskPageOffset(previous, next, at())); }));
  createEffect(() => { if (at() >= visible().length) setAt(Math.max(0, Math.floor((visible().length - 1) / TASK_PAGE) * TASK_PAGE)); });
  let revealed = "";
  createEffect(() => {
    const id = selectedTask(), task = id && id.project === project()?.id ? boardTask(rows(), sessionTaskRows(), id.uuid) : undefined;
    const key = id ? `${id.project}\n${id.uuid}` : "";
    if (!task || key === revealed) return;
    revealed = key;
    if (!(stage() === "current" && ["review", "active"].includes(task.stage))) setStage(task.stage as Stage);
    if (only() && !keepsSessionFilter(task, only()!, sessionTaskRows())) void setSetting("tasks/onlySession", false);
    if (task.stage === "done" && stageRows("done", true).some((t) => t.uuid === task.uuid)) setOlder(true);
  });
  const drag = () => draggedTask()?.project === project()?.id ? boardTask(rows(), sessionTaskRows(), draggedTask()?.uuid) : undefined;
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
  /** The same moves by a pointer drag where the webview gets no HTML5 drag events (lib/pointerDrag). */
  const pointerTask = (e: PointerEvent, t: Task) => {
    const projectId = project()?.id;
    if (!POINTER_DRAG || !projectId) return;
    const stop = () => { setDraggedTask(undefined); setDropTile(undefined); endDrag(); document.body.classList.remove("pointer-dragging"); };
    // A stage tile, the board's own list, or a session or project in the session list.
    const target = (x: number, y: number) => {
      const el = document.elementFromPoint(x, y);
      const tile = el?.closest<HTMLElement>(".tk-tile[data-filter]")?.dataset.filter as Filter | undefined;
      if (tile) return { tile };
      if (el && scroller.contains(el)) return { board: true };
      return { assign: taskAssignmentAt(el, projectId) };
    };
    pointerDrag(e, {
      start: () => {
        if (!canDrag(t) || writingTask() || tasks.project() !== projectId) return false;
        setDraggedTask({ project: projectId, uuid: t.uuid });
        document.body.classList.add("pointer-dragging");
        return true;
      },
      move: (x, y) => {
        const at = target(x, y);
        setDropTile(at.tile && !writingTask() && stageDropAction(drag(), at.tile) ? at.tile : undefined);
        if (!at.board || !canReorder()) { setLineAt(undefined); return; }
        const { elements, gap } = gapAt(y), edge = gap && elements[gap.before ? gap.gap : elements.length - 1];
        setLineAt(edge && gap ? edge.offsetTop + (gap.before ? -3 : edge.offsetHeight + 1) : undefined);
      },
      drop: (x, y) => {
        const at = target(x, y), task = drag();
        const action = at.tile && !writingTask() ? stageDropAction(task, at.tile) : undefined;
        const gap = at.board && canReorder() ? gapAt(y).gap : undefined;
        stop();
        if (!task) return;
        if (action) void actOnTask(projectId, task, action);
        else if (gap) void actOnTask(projectId, task, { kind: "reorder", anchor: gap.anchor, before: gap.before });
        else if (at.assign) void assign(at.assign.project, task, at.assign.row);
      },
      cancel: stop,
      scroller: () => scroller,
    });
  };
  const tileOver = (e: DragEvent, filter: Filter) => {
    if (!e.dataTransfer?.types.includes(TASK_MIME) || writingTask() || !stageDropAction(drag(), filter)) return false;
    e.preventDefault(); e.stopPropagation(); e.dataTransfer.dropEffect = "move"; endDrag(); return true;
  };
  const menu = (e: MouseEvent | KeyboardEvent, t: Task) => { e.preventDefault(); e.stopPropagation(); const anchor = e.currentTarget as HTMLElement, el = anchor.getBoundingClientRect(), projectId = project()!.id; setMenu({ x: el.left, y: el.bottom, items: [
    { label: tr("Task details"), icon: "file-text", action: () => openTask(projectId, t.uuid) },
    { label: tr("Rename…"), icon: "pencil", disabled: !t.writable || !!writingTask(), action: () => requestTaskRename(projectId, t.uuid) },
    ...boardActions(t.stage).map((action) => ({ label: actionLabel(action), icon: ACTION_ICON[action], disabled: !t.writable || !!writingTask(), action: () => void actOnTask(projectId, t, { kind: action }) })),
    ...(["todo", "backlog"].includes(t.stage) && supportsTaskStart(t) ? [{ label: tr("Start task"), icon: "play", disabled: !!taskStartBlocker(projectId, t) || !!writingTask(), hint: taskStartBlocker(projectId, t), action: () => void startTask(projectId, t) }] : []),
    ...(t.stage === "active" ? [{ label: tr("Cancel task…"), icon: "x", danger: true, disabled: !canCancel(t) || !!writingTask(), action: () => requestTaskCancellation(projectId, t, anchor) }] : []),
    { label: tr("Assign to session…"), icon: "user-round-cog", disabled: !canAssign(t, projectId) || !!assigningTask(), action: () => { if (project()?.id === projectId) openTaskAssignment(projectId, t, anchor); } },
  ] }); };
  return <div class="tk-panel"><Suspense><TaskCreate project={project()?.id} scope={project() ? `${visible().length} ${visible().length === 1 ? "task" : "tasks"}` : tr("Open a project session to see tasks")} accessible={!!project()?.accessible} backlog={stage() === "backlog"}>
    <button class="tk-sq" classList={{ on: flag("tasks/onlySession") }} aria-label={tr("Only this session")} title={tr("Only this session")} aria-pressed={flag("tasks/onlySession")} disabled={!project()} onClick={() => void setSetting("tasks/onlySession", !flag("tasks/onlySession"))}><Icon name="funnel" size={15} /></button>
    <button class="tk-sq" aria-label={tr("Task settings")} title={project() ? tr("Task settings for %1", project()!.name) : tr("Task settings")} disabled={!project()?.accessible || taskSettings.busy()} onClick={(e) => { const p = project(); if (p) void taskSettings.open(p.id, p.name, e.currentTarget); }}><Icon name="settings" size={15} /></button>
  </TaskCreate></Suspense>
    <Show when={project()} fallback={<div class="tk-empty tk-noproject"><Icon name="list-todo" size={22} /><span>{selected() && selected()!.state !== "archived" ? tr("Tasks belong to a project. Move this session into one to keep tasks for it.") : tr("Open a project session to see tasks")}</span></div>}>
      <div class="tk-tiles" role="tablist" aria-label={tr("Task stages")}><For each={FILTERS}>{(s) => <button class={`tk-tile st-${s}`} data-filter={s} classList={{ on: stage() === s, drop: dropTile() === s }} onDragOver={(e) => { if (tileOver(e, s)) setDropTile(s); }} onDragLeave={() => setDropTile(undefined)} onDrop={(e) => { setDropTile(undefined); if (!tileOver(e, s)) return; const task = readTaskDrag(e, project()!.id), action = stageDropAction(task, s); setDraggedTask(undefined); if (task && action) void actOnTask(project()!.id, task, action); }} role="tab" aria-label={`${tr(LABEL[s])} ${tileCount(s)}`} aria-selected={stage() === s} tabindex={stage() === s ? 0 : -1} onClick={() => setStage(s)} onKeyDown={(e) => { const i = FILTERS.indexOf(s), next = e.key === "ArrowRight" ? (i + 1) % 6 : e.key === "ArrowLeft" ? (i + 5) % 6 : e.key === "Home" ? 0 : e.key === "End" ? 5 : -1; if (next >= 0) { e.preventDefault(); setStage(FILTERS[next]); e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>("button")[next]?.focus(); } }}><span class="tk-tile-ico"><Icon name={ICON[s]} size={14} /></span><span class="tk-tile-label">{tr(LABEL[s])}</span><b class="tk-tile-count">{tileCount(s)}</b></button>}</For></div>
      <Show when={tasks.error()}><div class="notice danger tk-line" role="alert">Tasks could not be read: {tasks.error()}</div></Show>
      <Show when={tasks.damaged()}><div class="notice warn tk-line">{tasks.damaged()} task files of {project()!.name} could not be read.</div></Show>
      <Show when={tasks.keptDamaged().length || Object.keys(copyReviewOutcomes()).some((key) => key.startsWith(`${project()!.id}\n`))}><Suspense><TaskCopyReview project={project()!.id} name={project()!.name} /></Suspense></Show>
      <Show when={tasks.truncated()}><div class="notice warn tk-line">{tr("Showing the first 10,000 tasks.")}</div></Show>
      <Show when={pageError()}><div class="notice warn tk-line" role="status">{tr("More tasks could not be read: %1", pageError())}</div></Show>
      <Show when={drag() && canAssign(drag()!, project()!.id)}><div class="tk-drophint" role="status"><Icon name="corner-down-right" size={13} />{tr("Drop onto a session in the list to assign it; onto the project header for Any session.")}</div></Show>
      <div class="tk-scroll" ref={scroller} onDragOver={over} onDrop={dropped} onDragLeave={(e) => { if (!scroller.contains(e.relatedTarget as Node)) endDrag(); }}><div class="tk-cards" ref={cards} role="list" aria-label={tr("%1 tasks", tr(LABEL[stage()]))}><For each={shown()}>{(t) => <article class={`tk-card st-${t.stage}`} data-task={t.uuid} data-stage={t.stage} classList={{ sel: selectedTask()?.uuid === t.uuid, mine: !!selected() && belongsTo(t, selected()!), quiet: ["backlog", "done"].includes(t.stage), dragging: draggedTask()?.uuid === t.uuid }} tabindex="0" role="listitem" aria-label={`${t.id}: ${t.title}`} draggable={!POINTER_DRAG && canDrag(t) && !writingTask()} onDragStart={(e) => taskDragStart(e, t)} onPointerDown={(e) => pointerTask(e, t)} onDragEnd={() => { setDraggedTask(undefined); setDropTile(undefined); endDrag(); }} onClick={() => openTask(project()!.id, t.uuid)} onContextMenu={(e) => menu(e, t)} onKeyDown={(e) => { if (e.target !== e.currentTarget) return; if (e.key === "Enter" || e.key === " ") { e.preventDefault(); openTask(project()!.id, t.uuid); } else if (e.key === "ContextMenu" || e.shiftKey && e.key === "F10") menu(e, t); }}>
        <span class={`tk-glyph st-${t.stage}`} aria-hidden="true"><Show when={t.stage === "todo" && t.queue > 0}>{t.queue}</Show><Show when={["review", "done"].includes(t.stage)}><Icon name="check" size={11} /></Show></span>
        <div class="tk-card-main"><div class="tk-card-title">{t.title || tr("Untitled task")}</div><div class="tk-meta"><Show when={stage() === "current"}><span class="tk-meta-stage">{stageLabel(t.stage)}</span></Show><span class="mono tk-id">{t.id}</span><span class="tk-target ell" title={assignmentLabel(t)}>{t.worker ? `Subagent: ${t.worker}` : assignmentLabel(t)}</span><Show when={t.stage === "active" && find()(executorRef(t))}>{(s) => <span class={`hbadge t-${statusOf(s())}`}>{s().caption}</span>}</Show></div><Show when={t.stage === "review" && (t.summary || t.cancelReason)}><p class="tk-summary">{t.cancelReason || t.summary}</p></Show></div>
        <div class="tk-card-acts"><Show when={supportsTaskStart(t) && ["todo", "backlog"].includes(t.stage)}><TaskStartButton card project={project()!.id} task={t} showReason={showReason} /></Show><Show when={t.stage === "active"}><button class="icon-btn tk-act tk-primary blocked" aria-label={tr("Pause task")} aria-description={tr(PAUSE_REASON)} title={tr(PAUSE_REASON)} onClick={(e) => { e.stopPropagation(); openTask(project()!.id, t.uuid); showReason(e.currentTarget, tr(PAUSE_REASON)); }}><Icon name="pause" size={15} /></button></Show><Show when={primaryAction(t.stage)}>{(action) => <button class="icon-btn tk-act tk-primary" aria-label={actionLabel(action())} title={actionLabel(action())} disabled={!t.writable || !!writingTask()} onClick={(e) => { e.stopPropagation(); void actOnTask(project()!.id, t, { kind: action() }); }}><Icon name={ACTION_ICON[action()]} size={15} /></button>}</Show><button class="icon-btn tk-act" aria-label={tr("Task actions")} title={tr("Task actions")} aria-haspopup="menu" onClick={(e) => menu(e, t)}><Icon name="ellipsis" size={15} /></button></div>
      </article>}</For><div class="tk-dropline" classList={{ on: lineAt() !== undefined }} style={{ translate: `0 ${lineAt() ?? 0}px` }} aria-hidden="true" /></div><Show when={!visible().length && !tasks.error()}><div class="tk-empty"><Icon name={ICON[stage()]} size={20} /><span>{!project()!.accessible ? tr("This project is unavailable on this computer.") : listed() ? EMPTY[stage()] : tr("Loading tasks…")}</span></div></Show>
        <Show when={stage() === "done" && stageRows("done", true).length}><button class="tk-older" onClick={() => setOlder(!older())}>{older() ? tr("Hide older") : tr("Show older (%1)", stageRows("done", true).length)}</button></Show>
        <Show when={at() + TASK_PAGE >= visible().length && !tasks.error()}><For each={moreStages()}>{(s) => <Show when={!reading(s)} fallback={<div class="act-older hint tk-more-reading">{tr("Reading more tasks…")}</div>}>
          <button type="button" class="btn small ghost tk-more" onClick={() => void loadMore(s)}>{only() ? tr("%1 more in this stage", remaining(s)) : stage() === "current" ? tr("Show more: %1", stageLabel(s)) : tr("Show more")}</button>
        </Show>}</For></Show>
      </div>
      <Show when={visible().length > TASK_PAGE}><div class="tasks-navigation"><span class="muted">{at() + 1}–{Math.min(at() + TASK_PAGE, visible().length)} of {visible().length}</span><span class="grow" /><button class="btn small" disabled={!at()} onClick={() => setAt(Math.max(0, at() - TASK_PAGE))}>{tr("Previous")}</button><button class="btn small" disabled={at() + TASK_PAGE >= visible().length} onClick={() => setAt(at() + TASK_PAGE)}>{tr("Next")}</button></div></Show>
    </Show>
    <ReasonTip />
  </div>;
}
