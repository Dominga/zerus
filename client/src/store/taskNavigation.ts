import { createSignal, createEffect, createRoot } from "solid-js";
import { orderedRows } from "./catalog";
import { groupOf } from "./organization";
import { detailTab, page, selected, selectedKey, setPage, setSelectedKey, setDetailTab, setSubagentOpen, subagentOpen } from "./ui";
import { tasks } from "./tasks";
import { inspectorOpen, inspectorTab, openInspector } from "./inspector";
import { notify } from "./actions";
import { resolveTaskReference } from "../bridge";
import { openLinkedReference, SHORT_REFERENCE } from "./taskLinks";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export const [taskDescriptionRequest, setTaskDescriptionRequest] = createSignal<{ project: string; uuid: string }>();
export function requestTaskDescription(project: string, uuid: string) { openTask(project, uuid); setTaskDescriptionRequest({ project, uuid }); }
export const [taskRenameRequest, setTaskRenameRequest] = createSignal<{ project: string; uuid: string }>();
export function requestTaskRename(project: string, uuid: string) { openTask(project, uuid); setTaskRenameRequest({ project, uuid }); }
export const [selectedTask, setSelectedTask] = createSignal<{ project: string; uuid: string }>();
/** Task entry points share the session workspace; there is no separate Tasks page. */
export function openTasks(project?: string, sessionKey?: string, task?: string) {
  const current = selected();
  const target = sessionKey ? orderedRows().find((s) => s.key === sessionKey && (!project || groupOf(s.key) === project))
    : project && (!current || groupOf(current.key) !== project) ? orderedRows().find((s) => s.state !== "archived" && groupOf(s.key) === project) : current;
  if (target) { setSelectedKey(target.key); setSubagentOpen(undefined); }
  else if (project) setSelectedKey(undefined);
  setPage("sessions"); openInspector("tasks");
  if (task && project) { setSelectedTask({ project, uuid: task }); setDetailTab("taskview"); }
}
export function openTask(project: string, uuid: string) { setSelectedTask({ project, uuid }); setDetailTab("taskview"); openInspector("tasks"); }

/**
 * Whether the board is still as a link left it. Any navigation meanwhile
 * drops a late link result: another task, project, session, page, inspector
 * panel, detail tab or subagent.
 */
export function linkBoard(): () => boolean {
  const board = () => [selectedTask(), tasks.project(), selectedKey(), page(), inspectorOpen(), inspectorTab(), detailTab(), subagentOpen()];
  const before = board();
  return () => board().every((value, at) => value === before[at]);
}

const params = new URLSearchParams(location.search);
if (params.has("task") || params.get("page") === "tasks") createRoot((dispose) => {
  let opened = false;
  createEffect(() => {
    const project = params.get("project") ?? (selected() ? groupOf(selected()!.key) : undefined);
    const row = project ? orderedRows().find((s) => s.state !== "archived" && groupOf(s.key) === project) : selected();
    if (!row) return;
    if (!opened) { opened = true; openTasks(project, row.key); }
    const ref = params.get("task");
    if (!ref) { dispose(); return; }
    const task = tasks.project() === project ? tasks.tasks().find((t) => t.uuid === ref || t.id === ref || t.ref === ref) : undefined;
    if (task && project) { openTask(project, task.uuid); dispose(); }
    // A windowed list loads some rows of each stage: a task named by its UUID
    // opens anyway, and Task View reads it on its own. A short reference
    // beyond the window is resolved by one bounded read that checks it.
    else if (project && tasks.project() === project && tasks.windowed() && tasks.loaded()) {
      if (UUID.test(ref)) openTask(project, ref.toLowerCase());
      else if (SHORT_REFERENCE.test(ref)) {
        void openLinkedReference(project, ref, { resolve: resolveTaskReference, open: openTask, notify, unchanged: linkBoard() });
      }
      dispose();
    }
  });
});
