import { pendingExecutor, configureTaskExecutor, taskExecutors } from "./taskExecutors";
import { createSignal } from "solid-js";
import { taskStartOpen, taskStart, taskStartRelease, taskStartCheck, taskStartDiscardCheck } from "../bridge";
import { tasks } from "./tasks";
import { selected } from "./ui";
import { findTaskSession } from "./taskContext";
import { computerOffline } from "./catalog";
import { setWritingTask, writingTask } from "./taskBoard";
import { toast } from "./notices";
import { notify } from "./actions";
import { openTask } from "./taskNavigation";
import { sessionIdentity, sessionLabel, type Task } from "../ui/screens/tasks-ui";
import type { Session } from "../ui/sessions/types";
import { tr } from "../i18n";

export interface StartView { project: string; task: string; id: string; session: string; phase: string; snapshot: string; error: string; pending: boolean; outcome?: string | null; check_available?: boolean; unsent_check?: boolean; unsent_check_id?: string; check_problem?: string }
export interface StartInput { assignment: { kind: string; machine: string; session_id: string; name: string }; project: string; task: string; stage_at: number; target: { computer: string; name: string; run_id: string; conversation_id: string }; catalog_key: string; session_id: string }
export interface StartPort { open(project: string, task: string): Promise<StartView>; start(input: StartInput): Promise<StartView>; release(view: StartView): Promise<StartView>; check?(view: StartView): Promise<StartView>; discardCheck?(view: StartView): Promise<StartView> }
const scope = (v: { project: string; task: string } | undefined) => v ? `${v.project}\n${v.task}` : "";
export function createTaskStarts(port: StartPort) {
  const [view, setView] = createSignal<StartView>(), [error, setError] = createSignal("");
  let wanted: { project: string; task: string } | undefined, generation = 0, reading = false;
  const load = async () => {
    if (reading || !wanted) return;
    const target = { ...wanted }, mine = generation; reading = true;
    try { const answer = await port.open(target.project, target.task); if (mine === generation) { if (scope(answer) !== scope(target)) throw new Error("The saved task start did not match the selected task."); setView(answer); setError(""); } }
    catch (e) { if (mine === generation) setError(String(e)); }
    finally { reading = false; if (mine !== generation && wanted) void load(); }
  };
  const follow = (project?: string, task?: string) => {
    const next = project && task ? { project, task } : undefined;
    if (scope(next) === scope(wanted)) return;
    wanted = next; generation++; setView(undefined); setError(""); if (wanted) void load();
  };
  const accept = (answer: StartView) => { if (scope(wanted) === scope(answer)) { generation++; setView(answer); setError(""); } return answer; };
  return { view, error, follow, scope: () => wanted,
    start: async (input: StartInput) => { generation++; return accept(await port.start(input)); },
    release: async (snapshot: StartView) => { generation++; return accept(await port.release(snapshot)); },
    check: async (snapshot: StartView) => { if (!port.check) throw new Error("Checking the original receipt is unavailable."); generation++; const answer = await port.check(snapshot); if (answer.id !== snapshot.id) throw new Error("The saved task start changed. Refresh the task."); return accept(answer); },
    discardCheck: async (snapshot: StartView) => { if (!port.discardCheck) throw new Error("Discarding the unsent check is unavailable."); generation++; const answer = await port.discardCheck(snapshot); if (answer.id !== snapshot.id) throw new Error("The saved task start changed. Refresh the task."); return accept(answer); },
    refresh: () => { generation++; void load(); },
  };
}
export const taskStarts = createTaskStarts({ open: (project, task) => taskStartOpen(project, task), start: (input) => taskStart(input), release: (view) => taskStartRelease(view), check: (view) => taskStartCheck(view), discardCheck: (view) => taskStartDiscardCheck(view) });
export const [descriptionEditing, setDescriptionEditing] = createSignal<{ project: string; task: string; owner?: symbol }>();
export interface StartReview { task: Task; view: StartView; origin?: HTMLElement }
export const [taskStartReview, setTaskStartReview] = createSignal<StartReview>();
export const startReviewView = (review: StartReview) => { const current = taskStarts.view(); return current?.project === review.view.project && current.task === review.view.task && current.id === review.view.id ? current : review.view; };
export const startAssignment = (task: Task) => task.targetKind === "any" ? { kind: "any", machine: "", session_id: "", name: "" } : { kind: task.targetKind, machine: task.target.machine ?? "", session_id: task.target.id, name: task.target.id ? "" : task.target.name };
export const supportsTaskStart = (task: Task) => ["any", "session", "subagent", "new_session"].includes(task.targetKind);
export const startExecutor = (task: Task): Session | undefined => task.target.name ? findTaskSession()(task.target) : selected();
export const startView = (project: string, task: Task) => { const v = taskStarts.view(); return v?.project === project && v.task === task.uuid ? v : undefined; };
export const pendingStart = (project: string, task: Task) => startView(project, task)?.pending;
export const START_PENDING = "the previous start of this task may already have reached its executor.";
export function taskStartBlocker(project: string, task: Task): string | undefined {
  const launch = pendingExecutor(project, task.uuid);
  if (launch) return launch.phase === "waiting" ? tr("The new executor is starting") : tr("The previous executor launch needs review before another launch.");
  if (task.targetKind === "new_session" && taskExecutors.error()) return taskExecutors.error();
  if (pendingStart(project, task)) return START_PENDING;
  if (taskStarts.error() && taskStarts.scope()?.project === project && taskStarts.scope()?.task === task.uuid) return taskStarts.error();
  if (descriptionEditing()?.project === project && descriptionEditing()?.task === task.uuid) return tr("Save the description before starting this task");
  if (!task.writable) return tr("Update Zerus to edit this task");
  const any = !task.target.name, s = startExecutor(task);
  if (any && !s) return tr("Open the session to send this task to");
  if (!any && !task.target.known) return task.target.id ? `${sessionLabel(task.target)} no longer has a confirmed session identity — choose another session` : `${sessionLabel(task.target)} is on a computer this one has not verified`;
  if (!s) return `${sessionLabel(task.target)} no longer exists — choose another session`;
  if (task.targetKind === "new_session") return !task.target.id || !task.target.machine ? tr("Choose a parent with a confirmed computer and session identity.") : computerOffline(s.computer) ? tr("The selected session is offline.") : undefined;
  if (computerOffline(s.computer) || s.state !== "running" || s.processState === "exited") return tr("The agent is not running. Resume it before sending.");
  if (s.actionNeeded) return "agent is waiting for input, approval, or has an unknown state; open Terminal";
}
export function reviewTaskStart(project: string, task: Task, origin?: HTMLElement) {
  const view = startView(project, task); if (view?.pending) setTaskStartReview({ task: { ...task }, view: { ...view }, origin });
}
export async function startTask(project: string, task: Task) {
  if (writingTask()) return;
  const fresh = tasks.project() === project ? tasks.find(task.uuid) : undefined;
  if (!fresh || fresh.stageAt !== task.stageAt || fresh.stage !== task.stage || JSON.stringify(startAssignment(fresh)) !== JSON.stringify(startAssignment(task)) || !["todo", "backlog"].includes(fresh.stage) || !supportsTaskStart(fresh)) { notify("error", tr("The task or executor changed. Nothing was sent. Refresh the task.")); return; }
  openTask(project, task.uuid);
  const reason = taskStartBlocker(project, task), executor = startExecutor(task);
  if (reason || !executor) { notify("info", reason ?? tr("Open the session to send this task to")); return; }
  setWritingTask(task.uuid);
  try {
    const input = { project, task: task.uuid, assignment: startAssignment(task), stage_at: task.stageAt, target: { computer: executor.computer, name: executor.name, run_id: executor.runId ?? "", conversation_id: executor.conversationId ?? "" }, catalog_key: executor.catalogKey, session_id: sessionIdentity(executor) };
    if (task.targetKind === "new_session") { await configureTaskExecutor(input, executor.name.split("/")[0]); return; }
    const answer = await taskStarts.start(input);
    if (answer.outcome === "submitted") notify("info", tr("Task started."));
    else if (answer.outcome === "refused") notify("error", answer.error);
    // Nothing was reserved or sent; the executor's reason follows (A4).
    else if (answer.outcome === "cutting_over") notify("error", answer.error, true, undefined, tr("Not started: this task is moving to execution records. Nothing was sent; try again later."));
    // It may have reached the executor: reviewed, never sent again.
    else if (answer.pending) notify("error", answer.error || START_PENDING, true, undefined, tr("Start unconfirmed: check the executor’s session. The task will not be sent again automatically."));
  } catch (e) { notify("error", String(e)); taskStarts.refresh(); }
  finally { setWritingTask(undefined); }
}
export async function releaseTaskStart(review: StartReview): Promise<boolean> {
  if (writingTask() || startReviewView(review).phase === "delivering") return false;
  setWritingTask(review.task.uuid);
  try {
    const answer = await taskStarts.release(review.view);
    if (answer.outcome === "submitted") toast(tr("Delivery receipt found"), tr("The task reached its executor. Nothing was sent again."));
    else if (answer.outcome === "released") toast(tr("Start record released"), tr("Another start is allowed. Nothing was stopped or sent again."));
    else if (answer.outcome === "refused" && !answer.pending) notify("error", answer.error, true);
    else throw new Error("Unconfirmed release");
    return true;
  } catch { notify("error", tr("Could not release the start record."), true); return false; }
  finally { setWritingTask(undefined); }
}
export async function checkTaskStart(review: StartReview): Promise<boolean> {
  const view = startReviewView(review);
  if (writingTask() || !view.check_available || view.phase === "delivering") return false;
  setWritingTask(review.task.uuid);
  try {
    const answer = await taskStarts.check(view);
    if (answer.id !== view.id || answer.outcome !== "submitted" || answer.pending) throw new Error("The original receipt is still unconfirmed.");
    toast(tr("Delivery receipt found"), tr("The task reached its executor. Nothing was sent again.")); return true;
  } catch (e) { notify("error", String(e), true); taskStarts.refresh(); return false; }
  finally { setWritingTask(undefined); }
}
export async function discardTaskStartCheck(review: StartReview, expected?: StartView): Promise<boolean> {
  const view = expected ?? startReviewView(review);
  if (writingTask() || !view.unsent_check || !view.unsent_check_id) return false;
  setWritingTask(review.task.uuid);
  try {
    await taskStarts.discardCheck(view); return true;
  } catch (e) { notify("error", String(e), true); taskStarts.refresh(); return false; }
  finally { setWritingTask(undefined); }
}
