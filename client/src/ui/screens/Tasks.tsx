import { For, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { Icon } from "../lib/icons";
import { Body } from "../sessions/Activity";
import { statusOf } from "../sessions/SessionCard";
import { catalog, orderedRows } from "../../store/catalog";
import { groupOf, grouping, INBOX } from "../../store/organization";
import { taskProject, setTaskProject, taskSession } from "../../store/taskNavigation";
import { tasks } from "../../store/tasks";
import { flag, setSetting } from "../../store/settings";
import { pageHidden } from "../../store/visibility";
import { now, setPage, setSelectedKey, setDetailTab } from "../../store/ui";
import { STAGES, STAGE_ICON, STAGE_LABEL, TASK_PAGE, assignmentLabel, boardRows, executorRef, projectTask, taskPageOffset, taskSessions, type Stage, type Task } from "./tasks-ui";
import "../styles/tasks.css";

/** Qt's TaskBoard/TasksView, with the shared project list from the task stream. */
export function TasksScreen() {
  const projects = () => grouping().headers.filter((p) => p.id !== INBOX);
  const [stage, setStage] = createSignal<Stage>("todo");
  const [older, setOlder] = createSignal(false);
  const [at, setAt] = createSignal(0);
  const [selected, setSelected] = createSignal<string>();
  const project = () => projects().find((p) => p.id === taskProject());
  const sessions = createMemo(() => orderedRows().filter((s) => s.state !== "archived" && groupOf(s.key) === project()?.id));
  const find = createMemo(() => taskSessions(sessions()));
  const rows = createMemo(() => tasks.tasks().map((t) => projectTask(t, find())));
  const contextSession = () => { const s = taskSession() ? catalog.rows[taskSession()!] : undefined; return s && s.state !== "archived" && groupOf(s.key) === project()?.id ? s : undefined; };
  const only = () => flag("tasks/onlySession") ? contextSession() : undefined;
  const ageNow = createMemo(() => Math.floor(now() / 60) * 60_000);
  const stageRows = (s: Stage, old = false) => boardRows(rows(), s, only(), ageNow(), old);
  const visible = createMemo(() => tasks.error() ? [] : [...stageRows(stage()), ...(stage() === "done" && older() ? stageRows("done", true) : [])]);
  const shown = () => visible().slice(at(), at() + TASK_PAGE);
  const chosen = () => rows().find((t) => t.uuid === selected());
  let initialized = "";
  createEffect(() => {
    if (!taskProject() && projects().length) setTaskProject(projects().find((p) => p.id === grouping().default_project)?.id ?? projects()[0].id);
    tasks.follow(!pageHidden() && project()?.accessible ? project()!.id : undefined);
  });
  onCleanup(() => tasks.follow(undefined));
  createEffect(on(() => project()?.id, () => { initialized = ""; setSelected(undefined); setStage("todo"); setOlder(false); setAt(0); }));
  createEffect(() => {
    if (tasks.loaded() && !tasks.error() && tasks.project() && initialized !== tasks.project()) {
      initialized = tasks.project()!; setStage(stageRows("review").length ? "review" : "todo");
    }
  });
  createEffect(on(() => [stage(), only()?.key, older()] as const, () => setAt(0)));
  createEffect(on(visible, (next, previous) => { if (previous) setAt(taskPageOffset(previous, next, at())); }));
  createEffect(() => { if (at() >= visible().length) setAt(Math.max(0, Math.floor((visible().length - 1) / TASK_PAGE) * TASK_PAGE)); });
  const openExecutor = (task: Task) => {
    const s = find()(executorRef(task)); if (!s) return;
    setSelectedKey(s.key); setDetailTab("activity"); setPage("sessions");
  };
  const empty = () => stage() === "review" ? "Nothing to review" : stage() === "active" ? "No session is running a task"
    : stage() === "todo" ? "No tasks in To do" : stage() === "backlog" ? "Empty" : "Nothing yet";
  return <section class="su-page tasks-page" aria-label="Tasks">
    <header class="su-head tasks-head">
      <h1>Tasks</h1>
      <label class="sr-only" for="tasks-project">Project</label>
      <select id="tasks-project" class="field" aria-label="Project" value={project()?.id ?? ""} onChange={(e) => setTaskProject(e.currentTarget.value)}>
        <option value="" disabled>Select a project</option>
        <For each={projects()}>{(p) => <option value={p.id} disabled={!p.accessible} selected={p.id === project()?.id}>{p.name}</option>}</For>
      </select>
      <span class="grow" />
      <label class="check" title={contextSession() ? `Show only the tasks of ${contextSession()!.title}` : "Open Tasks from a session to filter its tasks."}>
        <input type="checkbox" disabled={!contextSession()} checked={!!only()} onChange={(e) => { void setSetting("tasks/onlySession", e.currentTarget.checked); }} />Only this session
      </label>
    </header>
    <Show when={project()} fallback={<p class="tasks-empty">Tasks belong to a project. Select a project to see its tasks.</p>}>
      <div class="tasks-stages" role="tablist" aria-label="Task stages">
        <For each={STAGES}>{(s) => <button type="button" role="tab" aria-selected={stage() === s} tabindex={stage() === s ? 0 : -1} class="btn tasks-stage" classList={{ on: stage() === s }}
          onClick={() => setStage(s)} onKeyDown={(e) => {
            const index = STAGES.indexOf(s), next = e.key === "ArrowRight" ? (index + 1) % STAGES.length : e.key === "ArrowLeft" ? (index + STAGES.length - 1) % STAGES.length
              : e.key === "Home" ? 0 : e.key === "End" ? STAGES.length - 1 : -1;
            if (next < 0) return; e.preventDefault(); setStage(STAGES[next]);
            e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>(".tasks-stage")[next]?.focus();
          }}><Icon name={STAGE_ICON[s]} size={18} /><span>{STAGE_LABEL[s]}</span><b>{stageRows(s).length}</b></button>}</For>
      </div>
      <Show when={!project()!.accessible}><p class="notice">This project is unavailable on this computer.</p></Show>
      <Show when={tasks.error()}>{(error) => <p class="notice danger" role="alert">Tasks could not be read: {error()}</p>}</Show>
      <Show when={tasks.damaged()}><p class="notice">{tasks.damaged()} task file(s) of {project()!.name} could not be read.</p></Show>
      <Show when={tasks.truncated()}><p class="notice">This project has more tasks than this view can retain. Showing the first 10,000.</p></Show>
      <div class="tasks-split">
        <div class="tasks-board">
          <div class="tasks-list" aria-label={`${STAGE_LABEL[stage()]} tasks`}>
            <For each={shown()}>{(t) => {
              const executor = () => find()(executorRef(t));
              return <button type="button" class="tasks-row" classList={{ on: selected() === t.uuid }} aria-pressed={selected() === t.uuid} onClick={() => setSelected(t.uuid)}>
                <Icon name={STAGE_ICON[t.stage as Stage] ?? "list-todo"} size={20} />
                <span class="tasks-row-body">
                  <span class="tasks-meta"><span class="mono">{t.id}</span><Show when={t.queue}><span>#{t.queue} in queue</span></Show>
                    <span class="ell">{t.worker ? `Subagent: ${t.worker}` : assignmentLabel(t)}</span>
                    <Show when={t.stage === "active" && executor()}><span class={`hbadge t-${statusOf(executor()!)}`}>{executor()!.caption}</span></Show>
                  </span>
                  <strong>{t.title || "Untitled task"}</strong>
                  <Show when={t.summary || t.cancelReason}><span class="tasks-excerpt">{t.cancelReason || t.summary}</span></Show>
                </span>
              </button>;
            }}</For>
            <Show when={!visible().length && !tasks.error()}><p class="tasks-empty">{!project()!.accessible ? "This project is unavailable on this computer." : tasks.loaded() ? empty() : "Loading tasks…"}</p></Show>
          </div>
          <div class="tasks-navigation">
            <Show when={stage() === "done" && stageRows("done", true).length}><button class="btn small" onClick={() => setOlder(!older())}>{older() ? "Hide older" : `Show older (${stageRows("done", true).length})`}</button></Show>
            <span class="grow" />
            <Show when={visible().length > TASK_PAGE}>
              <span class="muted">{at() + 1}–{Math.min(at() + TASK_PAGE, visible().length)} of {visible().length}</span>
              <button class="btn small" disabled={at() === 0} onClick={() => setAt(Math.max(0, at() - TASK_PAGE))}>Previous</button>
              <button class="btn small" disabled={at() + TASK_PAGE >= visible().length} onClick={() => setAt(at() + TASK_PAGE)}>Next</button>
            </Show>
          </div>
        </div>
        <aside class="tasks-detail" aria-label="Task details">
          <Show when={chosen()} fallback={<p class="tasks-empty">{selected() ? "This task is no longer listed." : "Select a task to see its details."}</p>}>{(t) => <>
            <div class="tasks-detail-head"><span class="mono">{t().id}</span><span>{STAGE_LABEL[t().stage as Stage] ?? t().stage}</span><span class="grow" />
              <button class="btn small" disabled={!find()(executorRef(t()))} onClick={() => openExecutor(t())}>Open executor</button></div>
            <h2>{t().title || "Untitled task"}</h2>
            <p class="muted">{t().stage === "todo" || t().stage === "backlog" ? "Planned" : t().stage === "active" ? "Executor" : "Last executor"}: {assignmentLabel(t())}</p>
            <Show when={t().summary || t().cancelReason}><article class="tasks-document"><h3>{t().cancelledAt ? "Cancelled" : "Result"}</h3><Body text={t().cancelReason || t().summary} /></article></Show>
            <article class="tasks-document"><h3>Description</h3><Body text={t().text || "No description yet."} /></article>
            <Show when={t().attachments}><p class="muted">Attachments: {t().attachments}</p></Show>
          </>}</Show>
        </aside>
      </div>
    </Show>
  </section>;
}
