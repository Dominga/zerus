import { createEffect, createMemo, onCleanup } from "solid-js";
import { orderedRows } from "./catalog";
import { groupOf, grouping, INBOX } from "./organization";
import { selected } from "./ui";
import { tasks } from "./tasks";
import { pageHidden } from "./visibility";
import { STAGES, projectTask, taskSessions, type Stage, type Task } from "../ui/screens/tasks-ui";

export const taskProject = () => { const s = selected(); return s && s.state !== "archived" ? grouping().headers.find((p) => p.id !== INBOX && p.id === groupOf(s.key)) : undefined; };
export const projectSessions = createMemo(() => orderedRows().filter((s) => s.state !== "archived" && groupOf(s.key) === taskProject()?.id));
export const findTaskSession = createMemo(() => taskSessions(projectSessions()));
export const projectTasks = createMemo(() => tasks.project() === taskProject()?.id ? tasks.tasks().map((t) => projectTask(t, findTaskSession())) : []);
/**
 * Only this session, as hgs pages it (zerus-cg5h.26): each stage's rows,
 * projected; undefined when the board filters its loaded rows instead. A To
 * do row keeps its place in the whole list when the window holds it, and has
 * none otherwise.
 */
export const sessionTaskRows = createMemo(() => {
  const view = tasks.session, find = findTaskSession();
  if (!view.active() || tasks.project() !== taskProject()?.id) return undefined;
  const queue = new Map(tasks.tasks().filter((t) => t.stage === "todo").map((t) => [t.uuid, t.queue]));
  return Object.fromEntries(STAGES.map((s) => [s, view.rows(s).map((t) => projectTask({ ...t, queue: queue.get(t.uuid) ?? 0 }, find))])) as Record<Stage, Task[]>;
});
export function followProjectTasks() {
  createEffect(() => tasks.follow(!pageHidden() && taskProject()?.accessible ? taskProject()!.id : undefined));
  onCleanup(() => tasks.follow(undefined));
}
