import { For, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { attachmentPin, attachmentUnpin, inTauri, openTaskAttachment } from "../../bridge";
import { notify } from "../../store/actions";
import { taskDetails, type TaskEntry } from "../../store/taskDetails";
import type { Fetched } from "../../types";
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
  return <article class="tasks-journal-entry">
    <div class="tasks-meta"><b>{props.entry.actor}</b><Show when={date()}>{(d) => <time datetime={d().toISOString()}>{d().toLocaleString()}</time>}</Show>
      <span>{label()}</span></div>
    <Body text={message() ? props.entry.text : historyAction(props.entry)} />
    <TaskFiles files={props.entry.files} />
    <Show when={props.discussion}><p class="muted">{deliveryText(props.entry, props.worker)}</p><Show when={deliveryError(props.entry)}><p class="notice">{deliveryError(props.entry)}</p></Show></Show>
  </article>;
}

const TABS = ["Overview", "Result", "History"] as const;
/** Qt's canonical document and result beside a bounded discussion/history. */
export function TaskInspector(props: { project: string; task: Task; presentation?: Task; executorAvailable: boolean; openExecutor: () => void }) {
  const [tab, setTab] = createSignal<typeof TABS[number]>("Overview"), [count, setCount] = createSignal(8);
  const identity = createMemo(() => `${props.project}\n${props.task.uuid}`);
  const revision = createMemo(() => JSON.stringify([props.project, props.task.uuid, props.task.historyRevision || props.task]));
  let selected = "";
  createEffect(on(revision, () => {
    const task = props.task, key = identity();
    if (key !== selected) { selected = key; taskDetails.select(props.project, task.uuid); }
    else taskDetails.refresh();
  }));
  createEffect(on(identity, () => { setTab("Overview"); setCount(8); }));
  onCleanup(() => taskDetails.select());
  const detail = taskDetails.data;
  const item = () => props.presentation ?? props.task;
  const shown = () => detail()?.log.slice(-count()) ?? [];
  const discussion = () => detail()?.log.filter((e) => e.kind === "message" || e.kind === "comment") ?? [];
  return <>
    <div class="tasks-detail-head"><span class="mono">{item().id}</span><span>{STAGE_LABEL[item().stage as Stage] ?? item().stage}</span><span class="grow" />
      <button class="btn small" disabled={!props.executorAvailable} onClick={props.openExecutor}>Open executor</button></div>
    <h2>{detail()?.title || item().title || "Untitled task"}</h2>
    <p class="muted">{item().stage === "todo" || item().stage === "backlog" ? "Planned" : item().stage === "active" ? "Executor" : "Last executor"}: {assignmentLabel(item())}</p>
    <div class="tasks-detail-tabs" role="tablist" aria-label="Task details">
      <For each={TABS}>{(name) => <button class="btn small" role="tab" tabindex={tab() === name ? 0 : -1} aria-selected={tab() === name} classList={{ on: tab() === name }} onClick={() => setTab(name)}
        onKeyDown={(e) => {
          const i = TABS.indexOf(name), next = e.key === "ArrowRight" ? (i + 1) % 3 : e.key === "ArrowLeft" ? (i + 2) % 3 : e.key === "Home" ? 0 : e.key === "End" ? 2 : -1;
          if (next < 0) return; e.preventDefault(); setTab(TABS[next]);
          e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>("button")[next]?.focus();
        }}>{name}</button>}</For>
    </div>
    <Show when={taskDetails.error()}><p class="notice danger" role="alert">Task could not be read: {taskDetails.error()}</p><button class="btn small" disabled={taskDetails.loading()} onClick={taskDetails.refresh}>Refresh</button></Show>
    <Show when={detail()} fallback={<p class="tasks-empty">{taskDetails.loading() ? "Loading task…" : "Task details are unavailable."}</p>}>{(d) => <>
      <Show when={d().stage === "active" && !d().started}><p class="notice">Dispatched to {assignmentLabel(d())} — start unconfirmed</p></Show>
      <Show when={tab() === "Overview"}>
        <Show when={(d().stage === "review" || d().stage === "done") && (d().summary || d().cancelReason)}><article class="tasks-document"><h3>{d().cancelledAt ? "Cancelled" : "Result"}</h3><Body text={d().cancelReason || d().summary} /></article></Show>
        <article class="tasks-document"><h3>Description</h3><Body text={d().text || "No description yet."} /><TaskFiles files={d().files} /></article>
        <h3>Discussion</h3>
        <For each={discussion()}>{(entry) => <JournalEntry entry={entry} worker={d().worker} discussion />}</For>
        <Show when={!discussion().length}><p class="muted">Messages and notes appear here.</p></Show>
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
  </>;
}
