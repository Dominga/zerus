import { createSignal } from "solid-js";
import { assignTask } from "../bridge";
import { catalog, computerFresh } from "./catalog";
import { groupOf } from "./organization";
import { tasks } from "./tasks";
import { notify } from "./actions";
import { sessionIdentity, type Task } from "../ui/screens/tasks-ui";
import { canDrag, writingTask, setWritingTask } from "./taskBoard";
import type { Session } from "../ui/sessions/types";
import { tr } from "../i18n";

export const TASK_MIME = "application/x-zerus-task";
export const [draggedTask, setDraggedTask] = createSignal<{ project: string; uuid: string }>();
export const assigningTask = writingTask, setAssigningTask = setWritingTask;
export function canAssign(task: Task, project: string, session?: Session) {
  return task.writable && ["todo", "backlog"].includes(task.stage) && !!project && (!session || session.state !== "archived" && session.tracked && computerFresh(session.computer) && !!sessionIdentity(session) && groupOf(session.key) === project);
}
export function taskDragStart(e: DragEvent, task: Task) {
  const project = tasks.project(); if (!project || !canDrag(task) || !e.dataTransfer) { e.preventDefault(); return; }
  setDraggedTask({ project, uuid: task.uuid }); e.dataTransfer.effectAllowed = "move";
  e.dataTransfer.setData(TASK_MIME, JSON.stringify({ project, uuid: task.uuid }));
}
export async function assign(project: string, task: Task, row?: Session, mode: "session" | "subagent" | "new_session" = "session") {
  if (assigningTask()) return;
  const freshTask = tasks.project() === project ? tasks.find(task.uuid) : undefined;
  const freshRow = row ? catalog.rows[row.key] : undefined;
  if (!freshTask || !canAssign(freshTask, project, freshRow) || row && (!freshRow || sessionIdentity(freshRow) !== sessionIdentity(row) || freshRow.runId !== row.runId || freshRow.conversationId !== row.conversationId)) { notify("error", tr("The task or session changed. Nothing was changed.")); return; }
  // Snapshot before awaiting: the catalog reconciles its rows in place.
  const target = freshRow ? { computer: freshRow.computer, name: freshRow.name, run_id: freshRow.runId ?? "", conversation_id: freshRow.conversationId ?? "" } : null;
  const key = freshRow?.catalogKey ?? null, id = freshRow ? sessionIdentity(freshRow) : null;
  setAssigningTask(task.uuid);
  try { if (mode === "session") await assignTask(project, task.uuid, target, key, id); else { if (!target) throw new Error("Choose a parent with a confirmed computer and session identity."); await assignTask(project, task.uuid, target, key, id, mode); } notify("info", target ? tr("Task assigned to %1", target.name) : tr("Task assigned to Any session")); }
  catch (error) { notify("error", String(error)); }
  finally { setAssigningTask(undefined); }
}
export function taskDragOver(e: DragEvent, project: string, row?: Session) {
  if (!e.dataTransfer?.types.includes(TASK_MIME)) return;
  e.stopPropagation();
  const drag = draggedTask(), task = drag && tasks.find(drag.uuid);
  if (drag?.project === project && task && canAssign(task, project, row)) { e.preventDefault(); e.dataTransfer.dropEffect = "move"; }
}
/** Accept only the exact drag started by this client, never foreign JSON. */
export function readTaskDrag(e: DragEvent, project: string) {
  if (!e.dataTransfer?.types.includes(TASK_MIME)) return;
  const drag = draggedTask(), bytes = e.dataTransfer.getData(TASK_MIME);
  if (!drag || drag.project !== project || tasks.project() !== project || bytes.length > 1024) return;
  try {
    const data = JSON.parse(bytes);
    if (data.project !== project || data.uuid !== drag.uuid) return;
    return tasks.find(data.uuid);
  } catch { /* A foreign or damaged drag is not a task write. */ }
}
export function taskDrop(e: DragEvent, project: string, row?: Session) {
  if (!e.dataTransfer?.types.includes(TASK_MIME)) return;
  e.preventDefault(); e.stopPropagation();
  const task = readTaskDrag(e, project); setDraggedTask(undefined);
  if (task) void assign(project, task, row);
}

/**
 * A pointer drag's assignment target in the session list (lib/pointerDrag):
 * the session or project header under `el`, when the dragged task may be
 * assigned there, as taskDragOver allows an HTML5 drop.
 */
export function taskAssignmentAt(el: Element | null | undefined, project: string): { project: string; row?: Session } | undefined {
  const key = el?.closest<HTMLElement>(".sp-scroll .vrow")?.dataset.key;
  const drag = draggedTask(), task = drag && tasks.find(drag.uuid);
  if (!key || !task || drag.project !== project || tasks.project() !== project) return;
  if (key.startsWith("h:")) return key.slice(2) === project && canAssign(task, project) ? { project } : undefined;
  const row = catalog.rows[key];
  return row && groupOf(row.key) === project && canAssign(task, project, row) ? { project, row } : undefined;
}

export const [taskAssignmentPopover, setTaskAssignmentPopover] = createSignal<{ project: string; task: Task; origin: HTMLElement }>();
export function openTaskAssignment(project: string, task: Task, origin: HTMLElement) {
  if (canAssign(task, project) && !writingTask()) setTaskAssignmentPopover({ project, task: { ...task, target: { ...task.target } }, origin });
}
