// Project task board model, shared by the Tasks inspector, the Task View and
// the task dialogs. Rules follow tray/src/TaskBoard.*, TasksView.* and
// TaskInspector.*: stage order, the Active union, actions per stage (primary
// last), blockers, the "Only this session" funnel, Done's 7-day window and the
// 100-card / 80-history bounds. In the Tauri client the listing comes from
// `hgs task list` through the Rust backend; here mutations edit a local store.
import { batch, createEffect, createMemo, createRoot, createSignal, on } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { catalog, contentScale, demoState, selected, selectedTask, setDetailTab, setDialog, setSelectedKey, setSelectedTask, stress, toast } from "../../state";
import { projects } from "../../mock/data";
import { damagedFiles, defaultSettings, hostOf, seedTasks, stressTasks, taskSettings, type TaskFile, type TaskLog, type TaskNote, type TaskSettings, type TaskStage, type TaskTarget, type TaskVM } from "../../mock/tasks-extra";
import type { Session } from "../../mock/types";
import { capture, flyToTile, reduced } from "./motion";

export type { TaskVM, TaskStage, TaskNote, TaskLog, TaskFile };
export type Filter = "current" | TaskStage;
export type Action = "edit" | "rename" | "toBacklog" | "top" | "sendNow" | "toTodo" | "cancel" | "backToTodo" | "accept" | "pause";

export const STAGES: TaskStage[] = ["review", "active", "todo", "backlog", "done"];
export const FILTERS: Filter[] = ["current", ...STAGES];
export const VISIBLE_TASKS = 100;
export const VISIBLE_HISTORY = 80;
export const HISTORY_PAGE = 8;
const RECENT_DONE_DAYS = 7;

export const STAGE_LABEL: Record<Filter, string> = { current: "Active", review: "In review", active: "In progress", todo: "To do", backlog: "Backlog", done: "Done" };
// A stage's icon is the icon of the action that moves a task there.
export const STAGE_ICON: Record<Filter, string> = { current: "activity", review: "inbox", active: "play", todo: "list-todo", backlog: "clock", done: "check-check" };

export const ACTION_LABEL: Record<Action, string> = {
  edit: "Task details", rename: "Rename…", toBacklog: "Move to Backlog", top: "Move to the top of To do", sendNow: "Start task",
  toTodo: "Move to To do", cancel: "Cancel task…", backToTodo: "Back to To do", accept: "Accept", pause: "Pause task",
};
export const ACTION_ICON: Record<Action, string> = {
  edit: "file-text", rename: "pencil", toBacklog: "clock", top: "arrow-up-to-line", sendNow: "play",
  toTodo: "list-todo", backToTodo: "undo-2", cancel: "x", accept: "check", pause: "pause",
};

/** The same actions in the same order on every surface; the main one is last. */
export function actionsOf(stage: TaskStage): Action[] {
  switch (stage) {
    case "todo": return ["edit", "rename", "toBacklog", "top", "sendNow"];
    case "backlog": return ["edit", "rename", "toTodo", "sendNow"];
    case "active": return ["rename", "cancel"];
    case "review": return ["rename", "backToTodo", "accept"];
    case "done": return ["rename", "backToTodo"];
  }
}
export function primaryOf(stage: TaskStage): Action {
  return stage === "todo" || stage === "backlog" ? "sendNow" : stage === "review" ? "accept" : stage === "active" ? "pause" : "backToTodo";
}
export const matchesFilter = (stage: TaskStage, filter: Filter) => (filter === "current" ? stage === "active" || stage === "review" : stage === filter);
export const queued = (stage: TaskStage) => stage === "todo" || stage === "backlog";
export const sessionLabel = (host: string | undefined, name: string | undefined) => (name ? (host ? `${name} @${host}` : name) : "");
export const displayTitle = (t: TaskVM) => t.title || t.text.split("\n")[0];

// ---------------------------------------------------------------- context

/** A session of the open project as Tasks sees it (SessionsWindow::updateTasks). */
export interface TaskSession { key: string; host: string; name: string; status: string; kind: string; blocker: string; running: boolean; working: boolean }

function asTaskSession(s: Session): TaskSession {
  const running = s.status !== "offline" && s.status !== "paused" && s.status !== "ended";
  const working = s.status === "working" || s.status === "background";
  let status = "Ready", kind = "neutral";
  switch (s.status) {
    case "working": status = "Working"; kind = "working"; break;
    case "background": status = "Background"; kind = "background"; break;
    case "attention": status = s.attention === "approval" ? "Needs approval" : "Needs input"; kind = "attention"; break;
    case "error": status = "Error"; kind = "error"; break;
    case "scheduled": status = "Scheduled"; kind = "scheduled"; break;
    case "offline": status = "Offline"; kind = "offline"; break;
    case "paused": status = "Paused"; kind = "paused"; break;
    default: break;
  }
  const blocker = !running ? "The agent is not running. Resume it before sending."
    : s.status === "attention" ? "agent is waiting for input, approval, or has an unknown state; open Terminal" : "";
  return { key: s.key, host: hostOf(s.machine), name: s.name, status, kind, blocker, running, working };
}

/** The open session's project and identity; empty project for Inbox or archived rows. */
export const context = createMemo(() => {
  const s = selected();
  if (!s || s.status === "ended" || s.key.includes("/")) return { project: "", projectName: "", host: "", session: "", key: s?.key ?? "", noProject: false };
  const p = projects.find((x) => x.id === s.project);
  // A live session outside every catalog project (Inbox) has no tasks.
  return { project: p ? p.id : "", projectName: p?.name ?? "", host: hostOf(s.machine), session: s.name, key: s.key, noProject: !p };
});

export const projectSessions = createMemo<TaskSession[]>(() => {
  const project = context().project;
  if (!project) return [];
  return catalog.rows.filter((r) => r.project === project && r.status !== "ended").map(asTaskSession);
});
const sessionIndex = createMemo(() => new Map(projectSessions().map((s) => [`${s.host}/${s.name}`, s])));
export const findSession = (host: string | undefined, name: string | undefined) => (name ? sessionIndex().get(`${host ?? ""}/${name}`) : undefined);

// ---------------------------------------------------------------- store

const [board, setBoard] = createStore<{ tasks: TaskVM[] }>({ tasks: seedTasks() });
export { board };
createRoot(() => createEffect(on(stress, (on) => {
  setBoard("tasks", (list) => on ? [...list, ...stressTasks(140)] : list.filter((t) => !t.id.startsWith("T-s")));
}, { defer: true })));

const params = new URLSearchParams(location.search);
/** Review-only demo switches: ?taskdemo=interrupted|describe|recording|rename|creating; ?taskpage=result|history. */
export const taskDemo = params.get("taskdemo") ?? "";

const readOnly = () => { try { return localStorage.getItem("tasks/onlySession") === "1"; } catch { return false; } };
export const [onlySession, setOnlySessionRaw] = createSignal(readOnly());
export function setOnlySession(on: boolean) {
  setOnlySessionRaw(on);
  try { localStorage.setItem("tasks/onlySession", on ? "1" : "0"); } catch { /* per-viewer convenience only */ }
}
export const [filter, setFilterRaw] = createSignal<Filter | undefined>();
export const [showOlder, setShowOlder] = createSignal(false);
export const [page, setPage] = createSignal<"task" | "result" | "history">((params.get("taskpage") as "result" | "history" | null) ?? "task");
export const [describing, setDescribing] = createSignal(taskDemo === "describe");
export const [descriptionDraft, setDescriptionDraft] = createSignal<string | undefined>();
export const [noteOnly, setNoteOnly] = createSignal(false);
export const [busy, setBusy] = createSignal(false);
export const [recording, setRecording] = createSignal(taskDemo === "recording");
export const [renaming, setRenaming] = createSignal(taskDemo === "rename");
export const [historyVisible, setHistoryVisible] = createSignal(HISTORY_PAGE);
export const [historyOlder, setHistoryOlder] = createSignal(false);
export const [cancelTarget, setCancelTarget] = createSignal<string | undefined>();
export const [recentFlight, setRecentFlight] = createSignal<{ id: string; stage: TaskStage; at: number } | undefined>();
export const scaleStyle = () => ({ "--content-scale": String(contentScale()) });

export const projectTasks = createMemo(() => board.tasks.filter((t) => t.project === context().project));
export const taskById = (id: string | undefined) => (id ? board.tasks.find((t) => t.id === id) : undefined);
export const queueOf = (t: TaskVM) => (t.stage === "todo" ? projectTasks().filter((x) => x.stage === "todo").findIndex((x) => x.id === t.id) + 1 : 0);

/** Listing state: error and loading come from the review bar's demo state. */
export const listing = createMemo(() => {
  const d = demoState();
  if (d === "error") return { error: "ssh build-01: connection timed out after 20 s", loading: false };
  return { error: "", loading: d === "loading" };
});
export const damaged = () => (listing().error ? 0 : damagedFiles[context().project] ?? 0);

// The open session's tasks: pinned to it in To do and Backlog, sent to it later.
export function belongsTo(t: TaskVM): boolean {
  const c = context();
  if (!c.session) return false;
  if (queued(t.stage)) return t.target.kind !== "any" && t.target.known !== false && (t.target.host ?? "") === c.host && t.target.session === c.session;
  return !!t.executor && t.executor.known !== false && t.executor.host === c.host && t.executor.name === c.session;
}
// "Only this session" keeps those and the Any session tasks in To do and Backlog.
export const keptFor = (t: TaskVM) => belongsTo(t) || (queued(t.stage) && t.target.kind === "any");
const recentDone = (t: TaskVM) => (t.doneDaysAgo ?? 0) <= RECENT_DONE_DAYS;

export function rowsOf(f: Filter, older = false, only = onlySession()): TaskVM[] {
  if (listing().error || listing().loading) return [];
  return projectTasks().filter((t) => matchesFilter(t.stage, f) && (!only || keptFor(t)) && (f !== "done" || recentDone(t) !== older));
}
export const counts = createMemo(() => {
  const out = {} as Record<Filter, number>;
  for (const f of FILTERS) out[f] = rowsOf(f).length;
  return out;
});
/** The Tasks tab label's counts: In review as an attention badge, then To do. */
export const tabCounts = createMemo(() => {
  if (listing().error) return { review: 0, todo: 0 };
  const list = projectTasks();
  return { review: list.filter((t) => t.stage === "review").length, todo: list.filter((t) => t.stage === "todo").length };
});

/** Default stage on the first load: Active when it has items, else review/todo, else the first non-empty stage. */
export const activeFilter = createMemo<Filter>(() => {
  const chosen = filter();
  if (chosen) return chosen;
  const c = counts();
  if (c.current) return "current";
  const first = c.review ? "review" : "todo";
  if (c[first]) return first;
  return STAGES.find((s) => c[s]) ?? "todo";
});

export function canLeave() { return !busy() && !recording(); }
export function setFilter(f: Filter) {
  if (f !== activeFilter() && !canLeave()) return;
  if (f !== activeFilter()) setShowOlder(false);
  setFilterRaw(f);
}

// ---------------------------------------------------------------- blockers

/** Why Start cannot deliver this task now, as hgs would say it (TasksView::sendBlocker). */
export function sendBlocker(t: TaskVM, target: TaskTarget = t.target): string {
  if (descriptionDraft() === t.id) return "Save the description before starting this task";
  if (target.kind === "new_session") {
    if (!target.session || target.known === false) return "Choose a parent with a confirmed computer and session identity";
    if (t.launch) return t.launch.phase === "waiting" ? "The new executor is starting" : "The previous executor launch needs review before another launch.";
    return "";
  }
  const c = context();
  const any = target.kind === "any";
  const host = any ? c.host : target.host ?? "";
  const name = any ? c.session : target.session;
  if (!name) return "Open the session to send this task to";
  const label = sessionLabel(host, name);
  if (!any && target.known === false) return target.idLost ? `${label} no longer has a confirmed session identity — choose another session` : `${label} is on a computer this one has not verified`;
  const found = findSession(host, name);
  if (!found) return `${label} no longer exists — choose another session`;
  return found.blocker;
}

export function blockedOf(t: TaskVM): Partial<Record<Action, string>> {
  const out: Partial<Record<Action, string>> = { pause: "Pausing an individual task is not available yet" };
  if (actionsOf(t.stage).includes("sendNow")) { const why = sendBlocker(t); if (why) out.sendNow = why; }
  if (!t.writable) for (const a of actionsOf(t.stage)) if (a !== "edit") out[a] = "Update Zerus to edit this task";
  return out;
}

// ---------------------------------------------------------------- labels

/** The card's target or worker label (TaskRow.cpp:187-218). */
export function targetText(t: TaskVM): string {
  const c = context();
  if (queued(t.stage)) {
    if (t.target.kind === "new_session") return "→ new session when started";
    if (t.target.kind === "subagent") return "→ new subagent";
    if (t.target.kind === "any") return "→ any session" + (c.session ? ` · now ${sessionLabel(c.host, c.session)}` : "");
    return "→ " + sessionLabel(t.target.host, t.target.session);
  }
  if (t.worker) return `Subagent: ${t.worker}`;
  if (t.target.kind === "subagent" && t.executor) return `coordinator ${sessionLabel(t.executor.host, t.executor.name)} · child unconfirmed`;
  return t.executor ? sessionLabel(t.executor.host, t.executor.name) : "";
}
export const ageText = (min = 0) => (min < 60 ? `${min} min` : min < 24 * 60 ? `${Math.floor(min / 60)} h` : `${Math.floor(min / 1440)} d`);
export const workerLabel = (t: TaskVM) => (t.executor ? sessionLabel(t.executor.host, t.executor.name) : "");
export const executorSession = (t: TaskVM) => {
  const sent = !queued(t.stage) && t.executor;
  return sent ? findSession(t.executor!.host, t.executor!.name) : t.target.kind === "any" ? undefined : findSession(t.target.host, t.target.session);
};

/** The executor field (TaskInspector.cpp:651-661). */
export function assignmentText(t: TaskVM): string {
  const c = context();
  if (queued(t.stage)) {
    const assigned = t.target.kind === "any" || !t.target.session ? "Any available session" : sessionLabel(t.target.host, t.target.session);
    const main = (t.target.host ?? "") === c.host && t.target.session === c.session;
    const label = t.target.kind === "new_session" ? "New session · create when started"
      : t.target.kind === "subagent" ? `Subagent · coordinate through ${assigned}`
      : main ? `Main session · ${assigned}` : assigned;
    return `Planned: ${label}`;
  }
  const w = workerLabel(t) || "Unconfirmed";
  return t.stage === "active" ? `Executor: ${w}` : `Last executor: ${w}`;
}

/** The context line under the header (TaskInspector.cpp:695-704). */
export function contextLine(t: TaskVM): string {
  let line: string;
  if (t.executor) {
    const w = workerLabel(t);
    line = t.stage === "active" && !t.startConfirmed ? `Dispatched to ${w} · start unconfirmed` : t.stage === "active" ? `Executor: ${w}` : `Last executor: ${w}`;
  } else line = `Assigned: ${t.target.kind === "any" || !t.target.session ? "Any session" : sessionLabel(t.target.host, t.target.session)}`;
  if (t.stage === "todo") line += ` · #${queueOf(t)} in queue`;
  if (t.worker) line = `Subagent: ${t.worker} · coordinated by ${workerLabel(t)}`;
  else if (t.target.kind === "subagent" && t.stage === "active") line = `Subagent requested · coordinator ${workerLabel(t)} · child unconfirmed`;
  return line;
}

export const DELIVERY_LABEL: Record<TaskNote["delivery"], string> = {
  note: "Saved note · not sent",
  queued: "Waiting for Start",
  "submitted-queue": "Included in Start request",
  submitted: "Submitted to executor",
  "submitted-coordinator": "Submitted to coordinator · child receipt unavailable",
  failed: "Saved in task · not sent",
  unconfirmed: "Saved in task · delivery unconfirmed",
};
export const IDENTITY_REFUSAL = "Your comment is saved here. The task’s previous executor could not be verified, so it was not sent. Open executor Activity to continue the discussion, or return the task to To do and start it with the current executor.";

/** One history line's action (taskHistoryAction, TaskDialogs.cpp:88-111). */
export function historyAction(e: TaskLog): string {
  switch (e.kind) {
    case "created": return "created the task";
    case "title": return `renamed the task: ${e.text}`;
    case "edited": return "edited the prompt";
    case "stage": return `moved it to ${STAGE_LABEL[(e.text ?? "todo") as TaskStage] ?? e.text}`;
    case "target": return `changed the session: ${e.text}`;
    case "sent": return `sent it to ${e.text}`;
    case "report": return `reported: ${e.text}`;
    case "cancelled": return "cancelled it";
    case "comment": return `commented: ${e.text}`;
    case "message_delivery": return `message ${e.text}`;
    case "resumed": return "requested changes and submitted feedback";
    case "started": return "started the task";
  }
}

// ---------------------------------------------------------------- mutations

const pad = (n: number) => String(n).padStart(2, "0");
export const nowLabel = () => { const d = new Date(); return `Oct 27 ${pad(d.getHours())}:${pad(d.getMinutes())}`; };
const update = (id: string, fn: (t: TaskVM) => void) => setBoard("tasks", (t) => t.id === id, produce(fn));
const log = (id: string, entry: Omit<TaskLog, "at">) => update(id, (t) => { t.log.push({ at: nowLabel(), ...entry }); if (t.logTotal) t.logTotal++; });

/** Opens a task in the central Task View (TaskInspector::openTask → taskViewRequested). */
export function openTask(id: string, opts: { describe?: boolean; rename?: boolean } = {}) {
  if (selectedTask() !== id && !canLeave()) return;
  const t = taskById(id);
  batch(() => {
    if (selectedTask() !== id) {
      setSelectedTask(id); setPage("task"); setHistoryVisible(HISTORY_PAGE); setHistoryOlder(false);
      setDescribing(false); setRenaming(false); setNoteOnly(t?.stage === "done");
    }
    if (t) {
      const f = activeFilter();
      if (f !== "current" || !matchesFilter(t.stage, f)) setFilterRaw(t.stage);
      if (!keptFor(t)) setOnlySession(false);
      if (t.stage === "done" && !recentDone(t)) setShowOlder(true);
    }
    if (opts.describe && t && t.writable && queued(t.stage)) { setDescribing(true); setPage("task"); }
    if (opts.rename && t?.writable) setRenaming(true);
    setDetailTab("taskview");
  });
}

/** Moves a task to another stage with motion: rows glide, a leaving card flies to its tile. */
export function moveStage(id: string, stage: TaskStage, extra?: (t: TaskVM) => void) {
  const t = taskById(id);
  if (!t || t.stage === stage) return;
  const leaves = !matchesFilter(stage, activeFilter());
  const play = capture();
  if (leaves) flyToTile(id, stage);
  update(id, (x) => {
    x.stage = stage;
    if (stage === "active") x.stageAgeMin = 0;
    if (stage === "done") x.doneDaysAgo = 0;
    extra?.(x);
  });
  setRecentFlight({ id, stage, at: Date.now() });
  play();
}

function reorderRank(id: string, anchor: string, before: boolean) {
  setBoard("tasks", (list) => {
    const from = list.findIndex((t) => t.id === id);
    if (from < 0) return list;
    const next = list.slice();
    const [moved] = next.splice(from, 1);
    let to = next.findIndex((t) => t.id === anchor);
    if (to < 0) return list;
    if (!before) to++;
    next.splice(to, 0, moved);
    return next;
  });
}

export function moveToTop(id: string) {
  const first = projectTasks().find((t) => t.stage === "todo");
  if (!first || first.id === id) return;
  const play = capture();
  reorderRank(id, first.id, true);
  play();
}

/** `move --before|--after`: every writable stage can be reordered within its own list. */
export function reorder(id: string, anchor: string, before: boolean) {
  const t = taskById(id), a = taskById(anchor);
  if (!t || !a || !t.writable || !a.writable || t.stage !== a.stage || id === anchor) return;
  const play = capture();
  reorderRank(id, anchor, before);
  play();
}

/** The To do and Backlog tiles take a task of this project from the other list. */
export const canStageByDrop = (t: TaskVM | undefined, stage: Filter) =>
  !!t && t.writable && queued(t.stage) && (stage === "todo" || stage === "backlog") && t.stage !== stage;

export function retarget(id: string, target: TaskTarget) {
  const t = taskById(id);
  if (!t || !queued(t.stage) || !t.writable) return;
  update(id, (x) => { x.target = target; });
  log(id, { actor: "You", kind: "target", text: target.kind === "any" ? "any" : target.kind === "new_session" ? `new session from ${sessionLabel(target.host, target.session)}` : target.kind === "subagent" ? `subagent of ${sessionLabel(target.host, target.session)}` : sessionLabel(target.host, target.session) });
}

export function rename(id: string, title: string): string | undefined {
  const value = title.trim();
  // eslint-disable-next-line no-control-regex
  if (!value || new TextEncoder().encode(value).length > 512 || /[\u0000-\u001f\u007f]/.test(value)) return "Enter a single-line title, up to 512 bytes. Enter saves; Escape cancels.";
  if (/\bfail\b/i.test(value)) return "Title was not saved. Your edit is kept. Try Enter again.\nssh build-01: connection timed out after 20 s";
  const t = taskById(id);
  if (!t || value === displayTitle(t)) return undefined;
  update(id, (x) => { x.title = value; });
  log(id, { actor: "You", kind: "title", text: value });
  return undefined;
}

let created = 0;
export function createCard(title: string): string | undefined {
  const value = title.trim();
  if (!value) return "";
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(value)) return "Use a single-line task title without control characters.";
  if (new TextEncoder().encode(value).length > 512) return "Use a shorter task title (up to 512 bytes).";
  const c = context();
  const id = `T-n${String(++created).padStart(5, "0")}`;
  const stage: TaskStage = activeFilter() === "backlog" ? "backlog" : "todo";
  const play = capture();
  setBoard("tasks", (list) => {
    // A new card goes to the end of its stage; adding never moves the viewport.
    const last = list.map((t) => t.project === c.project && t.stage === stage).lastIndexOf(true);
    const card: TaskVM = {
      id, project: c.project, title: value, text: "", stage, writable: true,
      target: c.session ? { kind: "session", host: c.host, session: c.session } : { kind: "any" },
      created: nowLabel(), files: [], discussion: [], log: [{ at: nowLabel(), actor: "You", kind: "created" }],
    };
    const next = list.slice();
    next.splice(last < 0 ? next.length : last + 1, 0, card);
    return next;
  });
  play();
  openTask(id, { describe: true });
  return undefined;
}

export function saveDescription(id: string, text: string, files: TaskFile[]) {
  update(id, (x) => { x.text = text; x.files = files; });
  log(id, { actor: "You", kind: "edited" });
  setDescribing(false); setDescriptionDraft(undefined);
}

/** A note saved in task history (`hgs task comment`); never delivered to the executor. */
export function addNote(id: string, text: string) {
  update(id, (x) => { x.discussion.push({ id: `c${Date.now()}`, author: "You", kind: "note", text, at: nowLabel(), delivery: "note" }); });
  log(id, { actor: "You", kind: "comment", text });
}

/** Discussion: queue | send | reopen | note (`hgs task message --mode`). */
export function addMessage(id: string, text: string, files: TaskFile[]) {
  const t = taskById(id);
  if (!t || !t.writable) return;
  const note = noteOnly();
  const mode = note ? "note" : t.stage === "active" ? "send" : t.stage === "review" || t.stage === "done" ? "reopen" : "queue";
  const entry: TaskNote = {
    id: `c${Date.now()}`, author: "You", kind: note ? "note" : "message", text, at: nowLabel(), files: files.length ? files : undefined,
    delivery: mode === "note" ? "note" : mode === "queue" ? "queued" : t.worker ? "submitted-coordinator" : "submitted",
  };
  if (mode === "send" && !t.startConfirmed && t.executor) { entry.delivery = "failed"; entry.error = "dispatch receipt"; }
  setBusy(true);
  update(id, (x) => { x.discussion.push(entry); });
  log(id, { actor: "You", kind: "comment", text });
  setTimeout(() => {
    setBusy(false);
    if (mode === "reopen") {
      log(id, { actor: "You", kind: "resumed" });
      moveStage(id, "active", (x) => { x.startConfirmed = false; x.summary = undefined; x.cancelledAt = undefined; x.cancelReason = undefined; });
      setNoteOnly(false);
    }
  }, reduced() ? 0 : 650);
}

export function startTask(id: string) {
  const t = taskById(id);
  if (!t) return;
  if (t.target.kind === "new_session") {
    toast("New session", `Start ${t.id} in a new session from ${sessionLabel(t.target.host, t.target.session)}; delivery waits for that exact launch.`);
    setDialog("new-session");
    return;
  }
  const c = context();
  const host = t.target.kind === "any" ? c.host : t.target.host ?? "";
  const name = t.target.kind === "any" ? c.session : t.target.session!;
  moveStage(id, "active", (x) => {
    x.executor = { host, name }; x.startConfirmed = false;
    x.discussion.forEach((n) => { if (n.delivery === "queued") n.delivery = "submitted-queue"; });
  });
  log(id, { actor: "You", kind: "sent", text: sessionLabel(host, name) });
  // The executor acknowledges with `hgs task start`; the label stays "start unconfirmed" until then.
  setTimeout(() => {
    const now = taskById(id);
    if (now?.stage !== "active" || now.startConfirmed) return;
    update(id, (x) => { x.startConfirmed = true; });
    log(id, { actor: sessionLabel(host, name), agent: true, kind: "started" });
  }, 2600);
}

export function cancelTask(id: string, reason: string) {
  const t = taskById(id);
  if (!t) return;
  const ran = t.executor ? findSession(t.executor.host, t.executor.name) : undefined;
  const d = new Date();
  moveStage(id, "review", (x) => { x.cancelledAt = `${pad(d.getHours())}:${pad(d.getMinutes())}`; x.cancelReason = reason; });
  log(id, { actor: "You", kind: "cancelled" });
  if (reason) {
    update(id, (x) => { x.discussion.push({ id: `c${Date.now()}`, author: "You", kind: "note", text: reason, at: nowLabel(), delivery: "note" }); });
    log(id, { actor: "You", kind: "comment", text: reason });
  }
  // The task is cancelled even when hgs refused the cancel message.
  if (!ran || !ran.running) toast("Cancel message not delivered", `${t.id} is cancelled; its session did not get the cancel message: the agent is not running.`);
}

export function checkRecovery(id: string) {
  setBusy(true);
  setTimeout(() => {
    setBusy(false);
    update(id, (x) => { x.recovery = false; x.discussion.forEach((n) => { if (n.delivery === "unconfirmed") n.delivery = "submitted"; }); });
    log(id, { actor: "You", kind: "message_delivery", text: "8c41f2e0 · submitted" });
    toast("Delivery receipt found", "The previous comment reached the executor. Nothing was sent again.");
  }, reduced() ? 0 : 700);
}
export function releaseRecovery(id: string) {
  update(id, (x) => { x.recovery = false; });
  toast("Recovery record released", "The comment stays in the task; nothing was cancelled or sent again.");
}
export function releaseLaunch(id: string) {
  update(id, (x) => { x.launch = undefined; });
}

/** One entry point for every surface (TasksView::act). */
export function act(t: TaskVM, action: Action) {
  const why = blockedOf(t)[action];
  if (action === "rename") { if (t.writable) openTask(t.id, { rename: true }); return; }
  if (action === "pause") return; // Task-scoped execution accounting must precede pause.
  if (action === "edit") { openTask(t.id, { describe: true }); return; }
  if (!t.writable || !actionsOf(t.stage).includes(action)) return;
  if (action === "cancel") { setCancelTarget(t.id); setDialog("task-cancel"); return; }
  if (action === "sendNow") { if (!why) startTask(t.id); return; }
  if (action === "top") { moveToTop(t.id); return; }
  const stage: TaskStage = action === "toBacklog" ? "backlog" : action === "accept" ? "done" : "todo";
  moveStage(t.id, stage, (x) => { if (stage === "todo") { x.cancelledAt = undefined; x.cancelReason = undefined; } });
  log(t.id, { actor: "You", kind: "stage", text: stage });
}

export function openExecutor(t: TaskVM) {
  const s = executorSession(t);
  if (!s) { toast("Session unavailable", "The task's session identity is no longer available."); return; }
  setSelectedKey(s.key);
  setDetailTab("activity");
}

export const settingsFor = (project: string): TaskSettings => taskSettings[project] ?? defaultSettings;

// A session switch starts the board fresh, as TaskInspector::setContext does.
createRoot(() => createEffect(on(() => context().key, () => {
  batch(() => { setFilterRaw(undefined); setShowOlder(false); setSelectedTask(undefined); setDescribing(false); setRenaming(false); });
}, { defer: true })));

/** Keeps the initial ?task= selection visible in the list. */
export function revealInitial() {
  const t = taskById(selectedTask());
  if (!t || t.project !== context().project) return;
  const f = activeFilter();
  if (f !== "current" || !matchesFilter(t.stage, f)) setFilterRaw(t.stage);
  if (t.stage === "done" && !recentDone(t)) setShowOlder(true);
  setNoteOnly(t.stage === "done");
}
