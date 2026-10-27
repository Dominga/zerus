import { For, Show, createMemo, createSignal } from "solid-js";
import { Confirm, Dialog } from "../components/Overlays";
import { Icon } from "../lib/icons";
import { dialog, selectedTask, setDialog, toast } from "../state";
import {
  ACTION_ICON, ACTION_LABEL, STAGE_LABEL, act, actionsOf, addNote, board, cancelTarget, cancelTask, context, displayTitle, findSession, historyAction,
  primaryOf, projectSessions, queued, releaseLaunch, releaseRecovery, retarget, saveDescription, sendBlocker, sessionLabel,
  settingsFor, taskById, workerLabel, type Action, type TaskFile, type TaskVM,
} from "../screens/tasks/board";
import { TaskGlyph } from "../screens/tasks/TaskCard";
import { TaskOverlays } from "../screens/tasks/overlays";
import type { TaskTarget } from "../mock/tasks-extra";
import "../styles/tasks.css";

// Every task dialog and confirmation (tray/src/TaskDialogs.cpp and the task
// parts of SessionsWindow.cpp). Dialog ids: task-details, task-cancel,
// task-settings, task-release-recovery, task-launch-review.

const subject = (want: (t: TaskVM) => boolean) => {
  const sel = taskById(selectedTask());
  if (sel && want(sel)) return sel;
  const project = context().project;
  return board.tasks.find((t) => t.project === project && want(t)) ?? board.tasks.find(want);
};

function StagePill(props: { task: TaskVM }) {
  return <span class={`tv-stage st-${props.task.stage}`}><TaskGlyph task={props.task} />{STAGE_LABEL[props.task.stage]}</span>;
}

// ---------------------------------------------------------------- Task details / Edit task

const sameTarget = (a: TaskTarget, b: TaskTarget) => a.kind === b.kind && (a.host ?? "") === (b.host ?? "") && (a.session ?? "") === (b.session ?? "");

/** The standalone task form (TaskDialog): description, attachments, assignment, history and notes. */
function TaskDetailsInner(props: { task: TaskVM }) {
  const t = () => props.task;
  const editable = () => t().writable && queued(t().stage);
  const [text, setText] = createSignal(t().text);
  const [files, setFiles] = createSignal<TaskFile[]>([...t().files]);
  const [target, setTarget] = createSignal<TaskTarget>({ ...t().target });
  const [note, setNote] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const empty = () => editable() && !text().trim();
  const sessions = () => projectSessions();
  // A pinned session that no longer exists stays a choice until it is changed.
  const choices = createMemo<TaskTarget[]>(() => {
    const list: TaskTarget[] = [{ kind: "any" }, ...sessions().map((s) => ({ kind: "session" as const, host: s.host, session: s.name }))];
    const own = t().target;
    if (own.kind !== "any" && !list.some((c) => sameTarget(c, own))) list.push({ ...own });
    return list;
  });
  const chipLabel = (c: TaskTarget) => c.kind === "any" ? "Any session" : c.kind === "new_session" ? `New session from ${sessionLabel(c.host, c.session)}` : c.kind === "subagent" ? `Subagent of ${sessionLabel(c.host, c.session)}` : sessionLabel(c.host, c.session);
  const stageActions = () => actionsOf(t().stage).filter((a) => a !== "edit" && a !== "rename");
  const last = () => stageActions()[stageActions().length - 1];
  const why = (a: Action) => !t().writable ? "Update Zerus to edit this task" : empty() ? "The description is empty." : a === "sendNow" ? sendBlocker(t(), target()) : primaryOf(t().stage) === a && a === "pause" ? "Pausing an individual task is not available yet" : "";
  const label = (a: Action) => editable() ? `Save description, attachments and assignment changes, then: ${ACTION_LABEL[a]}` : ACTION_LABEL[a];
  const save = () => {
    if (!editable()) return;
    if (text() !== t().text || files().length !== t().files.length) saveDescription(t().id, text(), files());
    if (!sameTarget(target(), t().target)) retarget(t().id, target());
  };
  const run = (a: Action) => {
    setBusy(true);
    save();
    setDialog(undefined);
    act(taskById(t().id)!, a);
  };
  const attach = () => {
    if (files().length >= 8) { setError("Attach up to 8 nonempty files: 10 MiB each, 20 MiB per task."); return; }
    const n = files().filter((f) => f.kind === "file").length + 1;
    const f: TaskFile = { ref: `[File #${n}]`, name: `attachment-${n}.txt`, size: "3 KB", kind: "file" };
    setFiles([...files(), f]); setText(`${text()} ${f.ref} `); setError("");
  };
  const remove = (f: TaskFile) => { setFiles(files().filter((x) => x !== f)); setText(text().replace(f.ref, "").replace(/ {2,}/g, " ")); };
  const saveNote = () => {
    const value = note().trim();
    if (!value || !t().writable) return;
    addNote(t().id, value);
    setNote("");
  };
  const started = () => t().log.some((e) => e.kind === "started");
  return (
    <Dialog id="task-details" title={editable() ? `Edit task ${t().id}` : `Task details ${t().id}`} width="760px" icon={editable() ? "pencil" : "file-text"}
      footer={<>
        <For each={stageActions()}>{(a) => (
          <button class="btn tv-cmd" classList={{ "is-primary": a === last() }} disabled={busy() || !!why(a)} title={why(a) || label(a)} onClick={() => run(a)}>
            <Icon name={ACTION_ICON[a]} size={13} />{ACTION_LABEL[a].replace("…", "")}
          </button>
        )}</For>
        <span class="spacer" />
        <Show when={editable()} fallback={<button class="btn primary" autofocus onClick={() => setDialog(undefined)}>Close</button>}>
          <button class="btn" onClick={() => setDialog(undefined)}>Cancel</button>
          <button class="btn primary" disabled={busy() || empty()} title={empty() ? "The description is empty." : "Save description, attachments and assignment"}
            onClick={() => { save(); setDialog(undefined); }}>Save changes</button>
        </Show>
      </>}>
      <div class="td-form">
        <div class="td-meta"><StagePill task={t()} /><b class="mono">{t().id}</b><span class="dim parts"><span>{t().author ? sessionLabel(t().author!.host, t().author!.name) : "You"}</span><span>{t().created}</span></span></div>
        <div class="td-title">{displayTitle(t())}</div>
        <label class="td-label" for="td-desc">Description</label>
        <Show when={!editable()}><div class="hint">{!t().writable ? "Update Zerus to edit this task." : "Description, attachments and assignment can be edited in To do or Backlog."}</div></Show>
        <textarea id="td-desc" class="field td-desc" rows={6} readOnly={!editable() || busy()} value={text()} onInput={(e) => setText(e.currentTarget.value)} aria-label="Task description" />
        <Show when={files().length}>
          <ul class="td-files">
            <For each={files()}>{(f) => (
              <li>
                <button class="att-chip grow" title="Open attached file" onClick={() => toast("Attachment", `Opens ${f.name} from the private task cache`)}>
                  <Icon name={f.kind === "image" ? "image" : "file-text"} size={14} /><span class="tv-ref">{f.ref}</span><span class="ell">{f.name}</span><span class="dim">{f.size}</span>
                </button>
                <Show when={editable()}><button class="btn small" onClick={() => remove(f)}>Remove</button></Show>
              </li>
            )}</For>
          </ul>
        </Show>
        <Show when={editable()}>
          <div class="row"><button class="btn small" onClick={attach}><Icon name="paperclip" size={12} />Attach files…</button><span class="hint">Attach up to 8 nonempty files: 10 MiB each, 20 MiB per task.</span></div>
        </Show>
        <Show when={t().stage === "review" && t().summary}>
          <article class="act-msg k-agent td-report"><header><b>{workerLabel(t())}</b><span class="chip">Report</span><span class="act-time">{t().reportedAt}</span></header><div class="act-body"><p>{t().summary}</p></div></article>
        </Show>
        <span class="td-label">Assigned to</span>
        <Show when={editable()} fallback={
          <>
            <div>{t().target.kind === "any" || !t().target.session ? "Any session" : sessionLabel(t().target.host, t().target.session)}</div>
            <Show when={t().executor}>
              <span class="td-label">{t().stage !== "active" ? "Last executor" : started() ? "Executor" : "Dispatched to (start unconfirmed)"}</span>
              <div>{workerLabel(t())}</div>
            </Show>
          </>
        }>
          <Show when={choices().length <= 60} fallback={
            <select class="field" aria-label="Assigned to" value={choices().findIndex((c) => sameTarget(c, target()))} onChange={(e) => setTarget(choices()[Number(e.currentTarget.value)])}>
              <For each={choices()}>{(c, i) => <option value={i()}>{chipLabel(c)}</option>}</For>
            </select>
          }>
            <div class="td-chips" role="radiogroup" aria-label="Assigned to">
              <For each={choices()}>{(c) => {
                const s = () => (c.kind === "session" ? findSession(c.host, c.session) : undefined);
                return (
                  <button class="td-chip" role="radio" aria-checked={sameTarget(c, target())} classList={{ on: sameTarget(c, target()), gone: c.kind === "session" && !s() }}
                    title={c.kind === "any" ? "Any session" : `${sessionLabel(c.host, c.session)}${s() ? `: ${s()!.status}` : ": no longer exists"}`} onClick={() => setTarget(c)}>
                    <Show when={c.kind !== "any"}><span class={`tk-dot k-${s()?.kind ?? "gone"}`} /></Show>
                    {chipLabel(c)}
                  </button>
                );
              }}</For>
            </div>
          </Show>
          <div class="hint">Or drag the task onto a session in the list; onto the project header for Any session.</div>
        </Show>
        <span class="td-label">History</span>
        <div class="td-history" role="log">
          <For each={t().log.slice(-80)} fallback={<div class="dim">No history yet.</div>}>{(e) => (
            <p class="parts"><b>{e.actor}</b><span class="dim">{e.at}</span><span>{historyAction(e)}</span></p>
          )}</For>
        </div>
        <div class="row td-note">
          <input class="field grow" placeholder="Task note (not sent to the executor)" title="Save this note in task history. It does not deliver a message to the executor."
            value={note()} disabled={!t().writable} onInput={(e) => setNote(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); e.stopPropagation(); saveNote(); } }} />
          <button class="btn" disabled={!t().writable || !note().trim()} onClick={saveNote}>Save note</button>
        </div>
        <Show when={error()}><div class="tv-note-error" role="alert">{error()}</div></Show>
      </div>
    </Dialog>
  );
}

// ---------------------------------------------------------------- Cancel task

function CancelInner(props: { task: TaskVM }) {
  const t = () => props.task;
  const [reason, setReason] = createSignal("");
  const ran = () => (t().executor ? findSession(t().executor!.host, t().executor!.name) : undefined);
  const session = () => workerLabel(t()) || "its session";
  return (
    <Dialog id="task-cancel" title={`Cancel ${t().id}?`} width="500px" icon="triangle-alert"
      footer={<>
        <button class="btn" autofocus onClick={() => setDialog(undefined)}>Keep running</button>
        <span class="spacer" />
        <button class="btn solid-danger" onClick={() => { setDialog(undefined); cancelTask(t().id, reason().trim()); }}><Icon name="x" size={13} />Cancel task</button>
      </>}>
      <div class="td-form">
        <p class="td-cancel-text">{t().text || displayTitle(t())}</p>
        <p>
          <b>{t().id}</b> moves to In review, and <b>{session()}</b> gets the cancel message.
          <Show when={!ran() || !ran()!.running}> The agent is not running, so hgs will refuse the message; the task is cancelled all the same.</Show>
          <Show when={ran()?.running && ran()?.working}> {session()} is working; it reads the message when its turn ends.</Show>
        </p>
        <label class="td-label" for="td-reason">Reason (optional) — saved as a comment on the task, not sent to the agent</label>
        <input id="td-reason" class="field" placeholder="Why it is cancelled" value={reason()} onInput={(e) => setReason(e.currentTarget.value)} />
      </div>
    </Dialog>
  );
}

// ---------------------------------------------------------------- Project task settings

const [savedSettings, setSavedSettings] = createSignal<Record<string, ReturnType<typeof settingsFor>>>({});

function SettingsInner() {
  const c = () => context();
  const base = () => savedSettings()[c().project] ?? settingsFor(c().project);
  const [task, setTask] = createSignal(base().task_template);
  const [cancel, setCancel] = createSignal(base().cancel_template);
  const [others, setOthers] = createSignal(base().agents_other);
  const [limit, setLimit] = createSignal(base().agent_limit);
  const [error, setError] = createSignal("");
  const save = () => {
    const next = { task_template: task(), cancel_template: cancel(), agents_other: others(), agent_limit: limit() };
    if (!Number.isInteger(limit()) || limit() < 0 || limit() > 1000) { setError("Limit tasks started by agents: use 0 to 1000 per hour."); return; }
    // Save sends only the changed fields with `settings --set`.
    const changed = (Object.keys(next) as (keyof typeof next)[]).filter((k) => next[k] !== base()[k]);
    if (changed.length) {
      setSavedSettings({ ...savedSettings(), [c().project]: next });
      toast("Task settings saved", `hgs task settings --set: ${changed.join(", ")}`);
    }
    setDialog(undefined);
  };
  return (
    <Dialog id="task-settings" title={`Project tasks: ${c().projectName || "Project"}`} width="620px" icon="settings"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" onClick={save}>Save</button></>}>
      <div class="td-form">
        <h3 class="td-heading">Service prompts</h3>
        <label class="td-label" for="ts-task">Task (Send now)</label>
        <textarea id="ts-task" class="field mono td-template" rows={8} value={task()} onInput={(e) => setTask(e.currentTarget.value)} spellcheck={false} />
        <label class="td-label" for="ts-cancel">Cancel message</label>
        <textarea id="ts-cancel" class="field mono td-template" rows={4} value={cancel()} onInput={(e) => setCancel(e.currentTarget.value)} spellcheck={false} />
        <div class="hint">Placeholders: <code>{"{ref}"}</code>, <code>{"{author}"}</code>, <code>{"{prompt}"}</code>, <code>{"{tools}"}</code>, <code>{"{history}"}</code></div>
        <h3 class="td-heading">Agents</h3>
        <label class="check"><input type="checkbox" checked={others()} onChange={(e) => setOthers(e.currentTarget.checked)} />Agents may start and cancel tasks in other sessions</label>
        <div class="row td-limit">
          <label for="ts-limit">Limit tasks started by agents</label>
          <input id="ts-limit" class="field tabular" type="number" min={0} max={1000} step={1} value={limit()} onInput={(e) => setLimit(Number(e.currentTarget.value))} />
          <span class="muted">per hour</span>
          <Show when={limit() === 0}><span class="chip">no limit</span></Show>
        </div>
        <Show when={error()}><div class="tv-note-error" role="alert">{error()}</div></Show>
      </div>
    </Dialog>
  );
}

// ---------------------------------------------------------------- Executor launch review

function LaunchReviewInner(props: { task: TaskVM }) {
  const t = () => props.task;
  const l = () => t().launch!;
  return (
    <Dialog id="task-launch-review" title="Task executor launch" width="520px" icon="info"
      footer={<>
        <button class="btn" disabled={l().phase === "waiting"} title={l().phase === "waiting" ? "The new executor is starting" : undefined}
          onClick={() => { releaseLaunch(t().id); setDialog(undefined); toast("Launch record released", "Another launch is allowed. Nothing was stopped or sent again."); }}>
          I reviewed it, allow another launch
        </button>
        <span class="spacer" />
        <button class="btn primary" autofocus onClick={() => setDialog(undefined)}>Close</button>
      </>}>
      <div class="td-form">
        <div class="kv"><span>Task</span><span><b class="mono">{t().id}</b> {displayTitle(t())}</span></div>
        <div class="kv"><span>Session</span><span class="mono">{l().session}</span></div>
        <div class="kv"><span>Launch</span><span class="mono">{l().launch}</span></div>
        <div class="kv"><span>State</span><span>{l().phase}</span></div>
        <Show when={l().error}><div class="notice warn"><Icon name="triangle-alert" size={13} /><span>{l().error}</span></div></Show>
        <p class="muted">Check the task history and this executor before allowing another launch. Releasing the launch record does not stop an agent or retry delivery.</p>
      </div>
    </Dialog>
  );
}

export function TaskDialogs() {
  const details = () => subject(() => true);
  const cancel = () => taskById(cancelTarget()) ?? subject((t) => t.stage === "active" && t.writable);
  const recovery = () => subject((t) => !!t.recovery);
  const launch = () => subject((t) => !!t.launch);
  return (
    <>
      <Show when={dialog() === "task-details" && details()} keyed>{(t) => <TaskDetailsInner task={t} />}</Show>
      <Show when={dialog() === "task-cancel" && cancel()} keyed>{(t) => <CancelInner task={t} />}</Show>
      <Show when={dialog() === "task-settings"}><SettingsInner /></Show>
      <Show when={dialog() === "task-launch-review" && launch()} keyed>{(t) => <LaunchReviewInner task={t} />}</Show>
      <Confirm id="task-release-recovery" title="Release message recovery" danger confirm="Release record"
        body={<p>The previous message may already have reached the executor. Releasing this local record neither cancels nor resends it. Check the task and executor Activity before sending another message.</p>}
        onConfirm={() => { const t = recovery(); if (t) releaseRecovery(t.id); }} />
      <TaskOverlays />
    </>
  );
}
