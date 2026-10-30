import { createSignal } from "solid-js";
import { groupOf } from "./organization";
import { selected, setPage } from "./ui";

export const [taskProject, setTaskProject] = createSignal<string>();
export const [taskSession, setTaskSession] = createSignal<string>();
export function openTasks(project?: string, sessionKey?: string) {
  const s = selected();
  setTaskProject(project ?? (s ? groupOf(s.key) : undefined));
  setTaskSession(sessionKey ?? s?.key);
  setPage("tasks");
}
