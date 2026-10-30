import { For } from "solid-js";
import { ACTION_ICON, ACTION_LABEL, actOnTask, boardActions, primaryAction, writingTask } from "../../store/taskBoard";
import type { Task } from "./tasks-ui";
import { Icon } from "../lib/icons";

/** The mockup's workflow commands, without the menu-only Move to top. */
export function TaskWorkflow(props: { project: string; task: Task }) {
  return <div class="tv-commands" role="group" aria-label="Task workflow"><For each={boardActions(props.task.stage).filter((a) => a !== "top")}>{(action) => <button class="btn small tv-cmd" classList={{ "is-primary": action === primaryAction(props.task.stage) }} aria-label={ACTION_LABEL[action]} title={ACTION_LABEL[action]} disabled={!props.task.writable || !!writingTask()} onClick={() => void actOnTask(props.project, props.task, { kind: action })}><Icon name={ACTION_ICON[action]} size={13} />{action === "to_backlog" ? "Backlog" : action === "accept" ? "Done" : "To do"}</button>}</For></div>;
}
