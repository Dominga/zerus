import { createSignal, createEffect, createRoot } from "solid-js";
import { orderedRows } from "./catalog";
import { groupOf } from "./organization";
import { selected, setPage, setSelectedKey, setDetailTab, setSubagentOpen } from "./ui";
import { tasks } from "./tasks";
import { openInspector } from "./inspector";

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
  });
});
