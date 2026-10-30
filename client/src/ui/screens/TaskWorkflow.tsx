import { supportsTaskStart } from "../../store/taskStart";
import { TaskStartButton } from "./TaskStartButton";
import { createTaskReason } from "./TaskReason";
import { canCancel, requestTaskCancellation, PAUSE_REASON } from "../../store/taskCancel";
import { For, Show, createEffect, on } from "solid-js";
import { ACTION_ICON, ACTION_LABEL, actOnTask, boardActions, primaryAction, writingTask } from "../../store/taskBoard";
import type { Task } from "./tasks-ui";
import { Icon } from "../lib/icons";

/** The mockup's workflow commands, without the menu-only Move to top. */
export function TaskWorkflow(props: { project: string; task: Task }) {
  const { show: showReason, clear: clearReason, Tip: ReasonTip } = createTaskReason();
  createEffect(on(() => `${props.project}\n${props.task.uuid}`, clearReason));
  return <><div class="tv-commands" role="group" aria-label="Task workflow"><For each={boardActions(props.task.stage).filter((a) => a !== "top")}>{(action) => <button class="btn small tv-cmd" classList={{ "is-primary": action === primaryAction(props.task.stage) }} aria-label={ACTION_LABEL[action]} title={ACTION_LABEL[action]} disabled={!props.task.writable || !!writingTask()} onClick={() => void actOnTask(props.project, props.task, { kind: action })}><Icon name={ACTION_ICON[action]} size={13} />{action === "to_backlog" ? "Backlog" : action === "accept" ? "Done" : "To do"}</button>}</For><Show when={supportsTaskStart(props.task) && ["todo", "backlog"].includes(props.task.stage)}><TaskStartButton project={props.project} task={props.task} showReason={showReason} /></Show><Show when={props.task.stage === "active"}><button class="btn small tv-cmd" aria-label="Cancel task…" title="Cancel task…" disabled={!canCancel(props.task) || !!writingTask()} onClick={(e) => requestTaskCancellation(props.project, props.task, e.currentTarget)}><Icon name="x" size={13} />Cancel task…</button><button class="btn small tv-cmd is-primary blocked" aria-label="Pause task" title={PAUSE_REASON} aria-disabled="true" onClick={(e) => showReason(e.currentTarget, PAUSE_REASON)}><Icon name="pause" size={13} />Pause task</button></Show></div><ReasonTip /></>;
}
