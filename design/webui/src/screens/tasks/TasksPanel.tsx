import { For, Show, createMemo, createSignal, onCleanup } from "solid-js";
import { Icon } from "../../lib/icons";
import { selectedTask, setDialog } from "../../state";
import {
  FILTERS, STAGE_ICON, STAGE_LABEL, VISIBLE_TASKS, activeFilter, canStageByDrop, context, counts, createCard, damaged, listing,
  matchesFilter, moveStage, onlySession, reorder, revealInitial, rowsOf, scaleStyle, sessionLabel, setFilter, setOnlySession, setShowOlder,
  showOlder, tabCounts, taskById, taskDemo, type Filter, type TaskStage,
} from "./board";
import { bumpTile } from "./motion";
import { TaskCard } from "./TaskCard";
import "../../styles/tasks.css";

const MIME = "application/x-zerus-task";

// Empty texts per stage (TasksView.cpp:318-322). The To do text names the +
// button: the inspector has no task composer below the list.
const EMPTY: Record<Filter, string> = {
  current: "No task is in progress or in review",
  review: "Nothing to review",
  active: "No session is running a task",
  todo: "Empty — add a task with +",
  backlog: "Empty",
  done: "Nothing yet",
};

/** The Tasks tab label badge: amber In review count, then a gray To do count (TaskTabBadge). */
export function TaskTabBadge() {
  return (
    <span class="tk-tabbadge" aria-label={`${tabCounts().review} to review, ${tabCounts().todo} to do`}>
      <Show when={tabCounts().review}><span class="tk-tb review">{tabCounts().review}</span></Show>
      <Show when={tabCounts().todo}><span class="tk-tb todo">{tabCounts().todo}</span></Show>
    </span>
  );
}

// New card title drafts survive Escape, per project (QSettings tasks/newCard/<project>/title).
const [drafts, setDrafts] = createSignal<Record<string, string>>(taskDemo === "interrupted" || taskDemo === "creating" ? { aurora: "Document cursor pagination for client authors" } : {});
const [creating, setCreating] = createSignal(taskDemo === "interrupted" || taskDemo === "creating");
const [createError, setCreateError] = createSignal(taskDemo === "interrupted" ? "Creation was interrupted. Press Enter to recover this same card." : "");
// A reserved creation request keeps its title read-only until Enter recovers that same card (never replayed as a new one).
const [reserved, setReserved] = createSignal(taskDemo === "interrupted");
const [drag, setDrag] = createSignal<{ id: string; stage: TaskStage } | undefined>();
const [dropTile, setDropTile] = createSignal<Filter | undefined>();

function Header() {
  let input: HTMLInputElement | undefined;
  const c = () => context();
  const total = () => rowsOf(activeFilter()).length + (showOlder() ? rowsOf("done", true).length : 0);
  const scope = () => !c().project ? "Open a project session to see tasks" : listing().loading ? ""
    : total() > VISIBLE_TASKS ? `${VISIBLE_TASKS} of ${total()} tasks` : total() === 1 ? "1 task" : `${total()} tasks`;
  const draft = () => drafts()[c().project] ?? "";
  const open = () => { setCreating(true); queueMicrotask(() => input?.focus()); };
  const submit = () => {
    const error = createCard(draft());
    if (error === "") return;
    if (error) { setCreateError(error); return; }
    setDrafts({ ...drafts(), [c().project]: "" }); setCreateError(""); setCreating(false); setReserved(false);
  };
  return (
    <div class="tk-head">
      <Show when={creating() && c().project} fallback={<span class="tk-scope" aria-live="polite">{scope()}</span>}>
        <input ref={input} class="field tk-newtitle" maxlength={512} placeholder="Task title · Enter to create" aria-label="New task title"
          value={draft()} readOnly={reserved()} autofocus onInput={(e) => setDrafts({ ...drafts(), [c().project]: e.currentTarget.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter") { e.preventDefault(); submit(); }
            else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setCreating(false); }
          }} />
      </Show>
      <button class="tk-sq add" title="New task" aria-label="New task" disabled={!c().project} onClick={open}><Icon name="plus" size={16} /></button>
      <button class="tk-sq" classList={{ on: onlySession() }} aria-pressed={onlySession()} disabled={!c().project}
        title={onlySession() ? `Show the tasks of every session of ${c().projectName}` : `Show only the tasks of ${sessionLabel(c().host, c().session)}`}
        aria-label={onlySession() ? `Show the tasks of every session of ${c().projectName}` : `Show only the tasks of ${sessionLabel(c().host, c().session)}`}
        onClick={() => setOnlySession(!onlySession())}><Icon name="funnel" size={15} /></button>
      <button class="tk-sq" title={`Task settings for ${c().projectName}`} aria-label={`Task settings for ${c().projectName}`} disabled={!c().project}
        onClick={() => setDialog("task-settings")}><Icon name="settings" size={15} /></button>
    </div>
  );
}

function Tiles() {
  const stageDrop = (f: Filter, e: DragEvent) => {
    const d = drag();
    if (!d || !canStageByDrop(taskById(d.id), f)) return false;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    return true;
  };
  return (
    <div class="tk-tiles" role="tablist" aria-label="Task stages">
      <For each={FILTERS}>{(f) => (
        <button class={`tk-tile st-${f}`} data-tile={f} role="tab" aria-selected={activeFilter() === f} title={STAGE_LABEL[f]}
          aria-label={`${STAGE_LABEL[f]}, ${counts()[f]}`} classList={{ on: activeFilter() === f, drop: dropTile() === f }}
          onClick={() => setFilter(f)}
          onDragOver={(e) => { if (stageDrop(f, e)) setDropTile(f); }}
          onDragLeave={() => { if (dropTile() === f) setDropTile(undefined); }}
          onDrop={(e) => {
            const d = drag();
            setDropTile(undefined);
            if (!d || !stageDrop(f, e)) return;
            moveStage(d.id, f as TaskStage);
            bumpTile(f);
          }}>
          <span class="tk-tile-ico"><Icon name={STAGE_ICON[f]} size={14} /></span>
          <span class="tk-tile-label">{STAGE_LABEL[f]}</span>
          <span class="tk-tile-count tabular">{listing().loading ? "–" : counts()[f]}</span>
        </button>
      )}</For>
    </div>
  );
}

function CardList() {
  let scroller!: HTMLDivElement;
  let cards!: HTMLDivElement;
  let timer: number | undefined;
  let step = 0;
  const [lineAt, setLineAt] = createSignal<number | undefined>();
  const all = createMemo(() => rowsOf(activeFilter()).concat(showOlder() && activeFilter() === "done" ? rowsOf("done", true) : []));
  // Bounded window: an explicit selection beyond it replaces the last card.
  const shown = createMemo(() => {
    const list = all();
    let out = list.slice(0, VISIBLE_TASKS);
    const sel = selectedTask();
    if (sel && list.length > VISIBLE_TASKS && !out.some((t) => t.id === sel)) {
      const extra = list.find((t) => t.id === sel);
      if (extra) out = [...out.slice(0, VISIBLE_TASKS - 1), extra];
    }
    return out;
  });
  const older = () => (activeFilter() === "done" ? rowsOf("done", true).length : 0);
  const hiddenByFunnel = () => onlySession() && rowsOf(activeFilter(), false, false).length > all().length;

  const rowsFor = (stage: string) => [...cards.querySelectorAll<HTMLElement>(`.tk-card[data-stage="${stage}"]`)];
  const gapAt = (y: number) => {
    const d = drag()!;
    const rows = rowsFor(d.stage);
    let gap = rows.length;
    for (let i = 0; i < rows.length; i++) { const r = rows[i].getBoundingClientRect(); if (y < r.top + r.height / 2) { gap = i; break; } }
    const from = rows.findIndex((r) => r.dataset.task === d.id);
    return { rows, gap: from >= 0 && (gap === from || gap === from + 1) ? -1 : gap };
  };
  const stopScroll = () => { clearInterval(timer); timer = undefined; step = 0; };
  const endDrag = () => { setLineAt(undefined); stopScroll(); };
  const canReorder = () => { const d = drag(); const t = d && taskById(d.id); return !!t && t.writable && matchesFilter(t.stage, activeFilter()); };
  const over = (e: DragEvent) => {
    if (!canReorder()) { endDrag(); return; }
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    const { rows, gap } = gapAt(e.clientY);
    if (gap < 0 || !rows.length) setLineAt(undefined);
    else {
      const edge = gap < rows.length ? rows[gap] : rows[rows.length - 1];
      setLineAt(gap < rows.length ? edge.offsetTop - 3 : edge.offsetTop + edge.offsetHeight + 1);
    }
    // Near the top or bottom edge the list scrolls that way, every 30 ms.
    const r = scroller.getBoundingClientRect();
    step = e.clientY < r.top + 28 ? -8 : e.clientY > r.bottom - 28 ? 8 : 0;
    if (!step) stopScroll(); else if (!timer) timer = window.setInterval(() => (scroller.scrollTop += step), 30);
  };
  const dropped = (e: DragEvent) => {
    const d = drag();
    if (!d || !canReorder()) return endDrag();
    e.preventDefault();
    const { rows, gap } = gapAt(e.clientY);
    endDrag();
    if (gap < 0 || !rows.length) return;
    const before = gap < rows.length;
    reorder(d.id, (before ? rows[gap] : rows[rows.length - 1]).dataset.task!, before);
  };
  onCleanup(stopScroll);

  const startDrag = (id: string, stage: TaskStage, e: DragEvent) => {
    const t = taskById(id);
    if (!t || !e.dataTransfer) return;
    const c = context();
    e.dataTransfer.effectAllowed = "move";
    // The same payload the session list reads to retarget a To do / Backlog task.
    e.dataTransfer.setData(MIME, JSON.stringify({ project: c.project, uuid: t.id, id: t.id, stage: t.stage, host: t.target.host ?? "", session: t.target.session ?? "", known: t.target.known !== false }));
    e.dataTransfer.setData("text/plain", `${t.id}: ${t.title}`);
    setDrag({ id, stage });
  };

  return (
    <div class="tk-scroll" ref={scroller} onDragOver={over} onDrop={dropped} onDragLeave={(e) => { if (!scroller.contains(e.relatedTarget as Node)) endDrag(); }}>
      <Show when={!listing().loading} fallback={
        <div class="tk-cards" aria-busy="true">
          <For each={[0, 1, 2, 3]}>{() => <div class="tk-skel"><span class="skeleton tk-skel-glyph" /><span class="col grow" style={{ gap: "6px" }}><span class="skeleton" style={{ height: "12px", width: "70%" }} /><span class="skeleton" style={{ height: "10px", width: "45%" }} /></span></div>}</For>
        </div>
      }>
        <Show when={shown().length || listing().error} fallback={
          <div class="tk-empty">
            <Icon name={STAGE_ICON[activeFilter()]} size={20} />
            <span>{hiddenByFunnel() ? `No tasks of ${sessionLabel(context().host, context().session)} here` : EMPTY[activeFilter()]}</span>
            <Show when={hiddenByFunnel()}><button class="btn small" onClick={() => setOnlySession(false)}>Show every session</button></Show>
          </div>
        }>
          <div class="tk-cards" ref={cards} role="list" aria-label={`${STAGE_LABEL[activeFilter()]} tasks`}>
            <For each={shown()}>{(t) => (
              <TaskCard task={t} selected={selectedTask() === t.id} showStage={activeFilter() === "current"} dragging={drag()?.id === t.id}
                onDragStart={(e) => startDrag(t.id, t.stage, e)} onDragEnd={() => { setDrag(undefined); setDropTile(undefined); endDrag(); }} />
            )}</For>
            <div class="tk-dropline" classList={{ on: lineAt() !== undefined }} style={{ translate: `0 ${lineAt() ?? 0}px` }} aria-hidden="true" />
          </div>
        </Show>
        <Show when={older()}>
          <button class="tk-older" onClick={() => setShowOlder(!showOlder())}>{showOlder() ? "Hide older" : `Show older (${older()})`}</button>
        </Show>
      </Show>
    </div>
  );
}

export function TasksPanel() {
  // Before the first render: the initial ?task= selection decides the stage filter.
  revealInitial();
  const c = () => context();
  return (
    <div class="tk-panel" style={scaleStyle()}>
      <Header />
      <Show when={createError() && c().project}><div class="tk-create-error" role="alert">{createError()}</div></Show>
      <Show when={c().project} fallback={
        <div class="tk-empty tk-noproject">
          <Icon name="list-todo" size={22} />
          <span>{c().noProject ? "Tasks belong to a project. Move this session into one to keep tasks for it." : "Open a project session to see tasks"}</span>
        </div>
      }>
        <Tiles />
        <Show when={listing().error}><div class="notice danger tk-line"><Icon name="circle-alert" size={14} /><span>Tasks could not be read: {listing().error}</span></div></Show>
        <Show when={damaged()}><div class="notice warn tk-line"><Icon name="triangle-alert" size={14} /><span>{damaged() === 1 ? `1 task file of ${c().projectName} could not be read.` : `${damaged()} task files of ${c().projectName} could not be read.`}</span></div></Show>
        <Show when={drag() && (drag()!.stage === "todo" || drag()!.stage === "backlog")}>
          <div class="tk-drophint" role="status"><Icon name="corner-down-right" size={13} />Drop onto a session in the list to assign it; onto the project header for Any session.</div>
        </Show>
        <CardList />
      </Show>
    </div>
  );
}
