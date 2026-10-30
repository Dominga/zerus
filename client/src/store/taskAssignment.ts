import { createSignal } from "solid-js";
import { assignTask } from "../bridge";
import { catalog, computerFresh } from "./catalog";
import { groupOf } from "./organization";
import { tasks } from "./tasks";
import { notify } from "./actions";
import { sessionIdentity, type Task } from "../ui/screens/tasks-ui";
import type { Session } from "../ui/sessions/types";

export const TASK_MIME = "application/x-zerus-task";
export const [draggedTask, setDraggedTask] = createSignal<{ project: string; uuid: string }>();
export const [assigningTask, setAssigningTask] = createSignal<string>();
export function canAssign(task: Task, project: string, session?: Session) {
  return task.writable && ["todo", "backlog"].includes(task.stage) && !!project && (!session || session.state !== "archived" && session.tracked && computerFresh(session.computer) && !!sessionIdentity(session) && groupOf(session.key) === project);
}
export function taskDragStart(e: DragEvent, task: Task) {
  const project = tasks.project(); if (!project || !canAssign(task, project) || !e.dataTransfer) { e.preventDefault(); return; }
  setDraggedTask({ project, uuid: task.uuid }); e.dataTransfer.effectAllowed = "move";
  e.dataTransfer.setData(TASK_MIME, JSON.stringify({ project, uuid: task.uuid }));
}
export async function assign(project: string, task: Task, row?: Session) {
  if (assigningTask()) return;
  const freshTask = tasks.project() === project ? tasks.tasks().find((t) => t.uuid === task.uuid) : undefined;
  const freshRow = row ? catalog.rows[row.key] : undefined;
  if (!freshTask || !canAssign(freshTask, project, freshRow) || row && (!freshRow || sessionIdentity(freshRow) !== sessionIdentity(row) || freshRow.runId !== row.runId || freshRow.conversationId !== row.conversationId)) { notify("error", "The task or session changed. Nothing was changed."); return; }
  // Snapshot before awaiting: the catalog reconciles its rows in place.
  const target = freshRow ? { computer: freshRow.computer, name: freshRow.name, run_id: freshRow.runId ?? "", conversation_id: freshRow.conversationId ?? "" } : null;
  const key = freshRow?.catalogKey ?? null, id = freshRow ? sessionIdentity(freshRow) : null;
  setAssigningTask(task.uuid);
  try { await assignTask(project, task.uuid, target, key, id); notify("info", target ? `Task assigned to ${target.name}` : "Task assigned to Any session"); }
  catch (error) { notify("error", String(error)); }
  finally { setAssigningTask(undefined); }
}
export function taskDragOver(e: DragEvent, project: string, row?: Session) {
  if (!e.dataTransfer?.types.includes(TASK_MIME)) return;
  e.stopPropagation();
  const drag = draggedTask(), task = drag && tasks.tasks().find((t) => t.uuid === drag.uuid);
  if (drag?.project === project && task && canAssign(task, project, row)) { e.preventDefault(); e.dataTransfer.dropEffect = "move"; }
}
export function taskDrop(e: DragEvent, project: string, row?: Session) {
  if (!e.dataTransfer?.types.includes(TASK_MIME)) return;
  e.preventDefault(); e.stopPropagation();
  const drag = draggedTask(); setDraggedTask(undefined);
  const bytes = e.dataTransfer.getData(TASK_MIME); if (!drag || bytes.length > 1024) return;
  try { const data = JSON.parse(bytes); if (data.project !== project || data.project !== drag.project || data.uuid !== drag.uuid) return;
    const task = tasks.tasks().find((t) => t.uuid === data.uuid); if (task) void assign(project, task, row);
  } catch { /* A foreign or damaged drag is not a task write. */ }
}
