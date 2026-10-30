import { createEffect, createMemo, onCleanup } from "solid-js";
import { orderedRows } from "./catalog";
import { groupOf, grouping, INBOX } from "./organization";
import { selected } from "./ui";
import { tasks } from "./tasks";
import { pageHidden } from "./visibility";
import { projectTask, taskSessions } from "../ui/screens/tasks-ui";

export const taskProject = () => { const s = selected(); return s && s.state !== "archived" ? grouping().headers.find((p) => p.id !== INBOX && p.id === groupOf(s.key)) : undefined; };
export const projectSessions = createMemo(() => orderedRows().filter((s) => s.state !== "archived" && groupOf(s.key) === taskProject()?.id));
export const findTaskSession = createMemo(() => taskSessions(projectSessions()));
export const projectTasks = createMemo(() => tasks.project() === taskProject()?.id ? tasks.tasks().map((t) => projectTask(t, findTaskSession())) : []);
export function followProjectTasks() {
  createEffect(() => tasks.follow(!pageHidden() && taskProject()?.accessible ? taskProject()!.id : undefined));
  onCleanup(() => tasks.follow(undefined));
}
