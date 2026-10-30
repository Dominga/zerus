import { PAUSE_REASON } from "../../store/taskCancel";
import { For, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { attachmentPin, attachmentUnpin, inTauri, openTaskAttachment } from "../../bridge";
import { notify } from "../../store/actions";
import { taskDetails, type TaskEntry } from "../../store/taskDetails";
import type { Fetched } from "../../types";
import { TaskWorkflow } from "./TaskWorkflow";
import { TaskTitle } from "./TaskTitle";
import { taskRenameRequest, setTaskRenameRequest, taskDescriptionRequest, setTaskDescriptionRequest } from "../../store/taskNavigation";
import { TaskInput } from "./TaskInput";
import { setMenu } from "../../store/ui";
import { Icon } from "../lib/icons";
import { Body } from "../sessions/Activity";
import { assignmentLabel, STAGE_LABEL, type Stage, type Task } from "./tasks-ui";
import { deliveryError, deliveryText, historyAction } from "./task-history-ui";

function TaskFiles(props: { files: Fetched[] }) {
  return <div class="tasks-files"><For each={props.files}>{(file) => {
    attachmentPin(file.path); onCleanup(() => attachmentUnpin(file.path));
    const open = async () => {
      try { const notice = await openTaskAttachment(file.path); if (notice) notify("info", notice); }
      catch (error) { notify("error", String(error)); }
    };
    return <button class="att-card" onClick={() => void open()} title={`Open ${file.name}`}>
      <Show when={file.thumbnail && inTauri()}><img class="tasks-file-preview" src={convertFileSrc(file.path, "zerus-file")} alt="" /></Show>
      <span>{file.name}</span><span class="muted">{Math.max(1, Math.ceil(file.bytes / 1024))} KiB</span><span>Open →</span>
    </button>;
  }}</For></div>;
}

function JournalEntry(props: { entry: TaskEntry; discussion?: boolean; worker: string }) {
  const message = () => ["comment", "message", "report"].includes(props.entry.kind);
  const label = () => props.entry.kind === "report" ? "Report" : !message() ? "" : !props.discussion ? "Comment" : props.entry.kind === "message" ? "Message" : "Note";
  const date = () => { const d = new Date(props.entry.at); return props.entry.at > 0 && Number.isFinite(d.getTime()) ? d : undefined; };
  if (!["message", "comment", "report"].includes(props.entry.kind)) return <div class="tv-event"><i aria-hidden="true" /><span><b>{props.entry.actor}</b> <time>{date()?.toLocaleString()}</time> {historyAction(props.entry)}</span></div>;
  return <article class="act-msg tv-entry" classList={{ "k-user": props.entry.actor === "You", "k-agent": props.entry.actor !== "You" }}>
    <header><b>{props.entry.actor}</b><Show when={date()}>{(d) => <time datetime={d().toISOString()}>{d().toLocaleString()}</time>}</Show>
      <span class="chip">{label()}</span></header>
    <div class="act-body"><Body text={message() ? props.entry.text : historyAction(props.entry)} /></div>
    <TaskFiles files={props.entry.files} />
    <Show when={props.discussion}><span class="tv-delivery">{deliveryText(props.entry, props.worker)}</span><Show when={deliveryError(props.entry)}><p class="notice">{deliveryError(props.entry)}</p></Show></Show>
  </article>;
}

const TABS = ["Task", "Result", "History"] as const;
/** Qt's canonical document and result beside a bounded discussion/history. */
export function TaskInspector(props: { project: string; task: Task; presentation?: Task; executorAvailable: boolean; openExecutor: () => void }) {
  const [describing, setDescribing] = createSignal(false), [renaming, setRenaming] = createSignal(false);
  const [tab, setTab] = createSignal<typeof TABS[number]>("Task"), [count, setCount] = createSignal(8);
  const identity = createMemo(() => `${props.project}\n${props.task.uuid}`);
  const revision = createMemo(() => JSON.stringify([props.project, props.task.uuid, props.task.historyRevision || props.task]));
  let selected = "";
  createEffect(on(revision, () => {
    const task = props.task, key = identity();
    if (key !== selected) { selected = key; taskDetails.select(props.project, task.uuid); }
    else taskDetails.refresh();
  }));
  createEffect(on(identity, () => { setTab("Task"); setCount(8); setDescribing(false); setRenaming(false); }));
  createEffect(() => { const request = taskRenameRequest(); if (request?.project === props.project && request.uuid === props.task.uuid) { setTaskRenameRequest(undefined); setRenaming(true); } });
  createEffect(() => { const request = taskDescriptionRequest(); if (request?.project === props.project && request.uuid === props.task.uuid) { setTaskDescriptionRequest(undefined); if (["todo", "backlog"].includes(props.task.stage)) setDescribing(true); } });
  onCleanup(() => taskDetails.select());
  const detail = taskDetails.data;
  const item = () => props.presentation ?? props.task;
  const shown = () => detail()?.log.slice(-count()) ?? [];
  const discussion = () => detail()?.log.filter((e) => e.kind === "message" || e.kind === "comment") ?? [];
  return <>
    <div class="tv-col"><header class="tv-head"><div class="tv-title-row"><TaskTitle project={props.project} task={item()} title={detail()?.title || item().title || "Untitled task"} renaming={renaming()} setRenaming={setRenaming} saved={taskDetails.refresh} /><button class="icon-btn tv-more" aria-label="Task actions" title="Task actions" aria-haspopup="menu" onClick={(e) => { const box = e.currentTarget.getBoundingClientRect(); setMenu({ x: box.left, y: box.bottom, items: [{ label: "Rename task", icon: "pencil", disabled: !item().writable, action: (): void => { setRenaming(true); } }, { separator: true }, ...TABS.map((name) => ({ label: name === "Task" ? "Task and discussion" : name, checked: tab() === name, action: () => setTab(name) }))] }); }}><Icon name="ellipsis" size={17} /></button></div>
      <div class="tv-context"><span class={`tv-stage st-${item().stage}`}><span class={`tk-glyph st-${item().stage}`} /><span>{STAGE_LABEL[item().stage as Stage] ?? item().stage}</span></span><span class="mono tv-id">{item().id}</span><button class="tchip tv-assign" disabled={!props.executorAvailable} onClick={props.openExecutor} title="Open executor Activity">{assignmentLabel(item())}</button><span class="grow" /><TaskWorkflow project={props.project} task={item()} /></div>
      <Show when={item().stage === "active"}><div class="tv-blocker quiet"><Icon name="info" size={13} /><span>{PAUSE_REASON}</span></div></Show>
    </header></div><div class="tv-scroll"><div class="tv-col tv-content">
    <Show when={tab() !== "Task"}><div class="tv-section"><span class="eyebrow">{tab()}</span><span class="grow" /><button class="link tv-back" onClick={() => setTab("Task")}><Icon name="chevron-left" size={12} />Task and discussion</button></div></Show>
    <Show when={taskDetails.error()}><p class="notice danger" role="alert">Task could not be read: {taskDetails.error()}</p><button class="btn small" disabled={taskDetails.loading()} onClick={taskDetails.refresh}>Refresh</button></Show>
    <Show when={detail()} fallback={<p class="tasks-empty">{taskDetails.loading() ? "Loading task…" : "Task details are unavailable."}</p>}>{(d) => <>
      <Show when={d().stage === "active" && !d().started}><p class="notice">Dispatched to {assignmentLabel(d())} — start unconfirmed</p></Show>
      <Show when={tab() === "Task"}>
        <Show when={(d().stage === "review" || d().stage === "done") && (d().summary || d().cancelReason)}><article class="act-msg tv-entry tv-result k-agent"><header><b>{item().worker || "Agent"}</b><span class="chip">{d().cancelledAt ? "Cancelled" : "Result"}</span></header><div class="act-body"><Body text={d().cancelReason || d().summary} /></div></article></Show>
        <section class="tv-doc" classList={{ editing: describing() }} aria-label="Description"><header><span class="eyebrow">Description</span><span class="grow" /><Show when={d().writable && ["todo", "backlog"].includes(d().stage)}><Show when={describing()} fallback={<button class="btn small ghost" onClick={() => setDescribing(true)}><Icon name="pencil" size={12} />Edit description</button>}><span class="tv-doc-state"><Icon name="pencil" size={11} />Being edited below</span></Show></Show></header><div class="act-body"><Body text={d().text || "No description yet."} /><TaskFiles files={d().files} /></div></section>
        <div class="tv-section"><span class="eyebrow">Discussion</span></div>
        <For each={discussion()}>{(entry) => <JournalEntry entry={entry} worker={d().worker} discussion />}</For>
        <Show when={!discussion().length}><div class="tv-placeholder">Messages and notes appear here.</div></Show>
        <Show when={d().total > d().log.length}><p class="muted">Showing recent discussion. Earlier records are available in History.</p></Show>
      </Show>
      <Show when={tab() === "Result"}><article class="tasks-document"><h3>{d().cancelledAt ? "Cancelled" : "Report"}</h3><Body text={d().cancelReason || d().summary || "No result reported yet."} /></article></Show>
      <Show when={tab() === "History"}>
        <div class="tasks-navigation"><Show when={count() < d().log.length || d().next}><button class="btn small" disabled={taskDetails.loading()} onClick={() => {
          if (count() < d().log.length) setCount(count() + 8); else { setCount(8); taskDetails.older(); }
        }}>{count() < d().log.length ? `Earlier events (${Math.max(0, d().log.length - count())})` : "Earlier events…"}</button></Show>
          <span class="grow" /><Show when={taskDetails.before()}><button class="btn small" disabled={taskDetails.loading()} onClick={() => { setCount(8); taskDetails.latest(); }}>Latest</button></Show></div>
        <For each={shown()}>{(entry) => <JournalEntry entry={entry} worker={d().worker} />}</For>
        <Show when={!d().log.length}><p class="tasks-empty">No history yet.</p></Show>
      </Show>
    </>}</Show>
  </div></div><TaskInput project={props.project} task={item()} describing={describing()} cancel={() => setDescribing(false)} saved={() => { setDescribing(false); taskDetails.refresh(); }} /></>;
}
