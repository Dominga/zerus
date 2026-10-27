import { For, Match, Show, Switch, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { Icon } from "../../lib/icons";
import { demoState, selectedTask, setDialog, toast } from "../../state";
import {
  ACTION_ICON, ACTION_LABEL, DELIVERY_LABEL, HISTORY_PAGE, IDENTITY_REFUSAL, STAGE_LABEL, act, actionsOf, assignmentText, blockedOf, busy, canLeave,
  context, contextLine, describing, displayTitle, executorSession, historyAction, historyOlder, historyVisible, openExecutor, openTask, page,
  primaryOf, queued, recentFlight, rename, renaming, scaleStyle, setCancelTarget, setHistoryOlder, setHistoryVisible, setPage, setRenaming,
  taskById, taskDemo, workerLabel, type Action, type TaskFile, type TaskLog, type TaskNote, type TaskVM,
} from "./board";
import { TaskGlyph } from "./TaskCard";
import { TaskComposer } from "./TaskComposer";
import { below, openTaskMenu, setAssignMenu, showReason } from "./overlays";
import "../../styles/tasks.css";

/** Minimal mock formatter: attachment references, `code`, **bold**, example.com links. */
const TOKENS = /(\[(?:File|Image) #\d+\]|`[^`]+`|\*\*[^*]+\*\*|https:\/\/[a-z.]*example\.com\S*)/g;
function Rich(props: { text: string }) {
  return (
    <For each={props.text.split("\n\n")}>{(para) => (
      <p>{para.split(TOKENS).map((p) =>
        /^\[(File|Image) #\d+\]$/.test(p) ? <span class="tv-ref">{p}</span> :
        p.startsWith("`") ? <code>{p.slice(1, -1)}</code> :
        p.startsWith("**") ? <b>{p.slice(2, -2)}</b> :
        p.startsWith("https://") ? <a href="#" onClick={(e) => e.preventDefault()} title="Opens in the browser">{p}</a> : p)}</p>
    )}</For>
  );
}

function Files(props: { files?: TaskFile[] }) {
  return (
    <Show when={props.files?.length}>
      <div class="act-attachments">
        <For each={props.files}>{(f) => (
          <button class="att-chip" title="Open attached file" onClick={() => toast("Attachment", `Opens ${f.name} from the private task cache`)}>
            <Icon name={f.kind === "image" ? "image" : "file-text"} size={14} /><span class="tv-ref">{f.ref}</span><span>{f.name}</span><span class="dim">{f.size}</span>
          </button>
        )}</For>
      </div>
    </Show>
  );
}

const DELIVERY_ICON: Record<TaskNote["delivery"], string> = {
  note: "file-text", queued: "clock", "submitted-queue": "check", submitted: "check", "submitted-coordinator": "check", failed: "circle-alert", unconfirmed: "triangle-alert",
};

function Note(props: { note: TaskNote }) {
  const n = () => props.note;
  const refusal = () => n().error && /execution changed|identity changed|conversation changed|dispatch receipt|dispatched run/.test(n().error!) ? IDENTITY_REFUSAL : n().error;
  return (
    <article class="act-msg tv-entry" classList={{ "k-user": !n().agent, "k-agent": !!n().agent }}>
      <header>
        <b>{n().author}</b>
        <span class="chip">{n().kind === "note" ? "Note" : "Message"}</span>
        <span class="act-time">{n().at}</span>
        <span class="grow" />
        <span class={`tv-delivery d-${n().delivery}`}><Icon name={DELIVERY_ICON[n().delivery]} size={11} />{DELIVERY_LABEL[n().delivery]}</span>
      </header>
      <div class="act-body"><Rich text={n().text} /><Files files={n().files} /></div>
      <Show when={refusal()}><div class="notice warn tv-refusal"><Icon name="triangle-alert" size={13} /><span>{refusal()}</span></div></Show>
    </article>
  );
}

function ResultEntry(props: { task: TaskVM }) {
  const t = () => props.task;
  return (
    <article class="act-msg tv-entry tv-result" classList={{ "k-agent": !t().cancelledAt, cancelled: !!t().cancelledAt }}>
      <header>
        <b>{t().cancelledAt ? "You" : workerLabel(t())}</b>
        <span class="chip">{page() === "result" ? (t().cancelledAt ? "Cancelled" : "Report") : "Result"}</span>
        <span class="act-time">{t().cancelledAt ? `Oct 27 ${t().cancelledAt}` : t().reportedAt ?? ""}</span>
      </header>
      <div class="act-body"><Rich text={t().cancelledAt ? t().cancelReason || "Cancelled." : t().summary ?? ""} /><Show when={t().summary && !t().cancelledAt}><Files files={t().files.filter((f) => t().summary!.includes(f.ref))} /></Show></div>
    </article>
  );
}

function Description(props: { task: TaskVM }) {
  const t = () => props.task;
  const editable = () => t().writable && queued(t().stage);
  return (
    <section class="tv-doc" classList={{ editing: describing() && editable() }} aria-label="Description">
      <header>
        <span class="eyebrow">Description</span>
        <span class="grow" />
        <Show when={describing() && editable()} fallback={
          <Show when={editable()}><button class="btn small ghost" onClick={() => openTask(t().id, { describe: true })}><Icon name="pencil" size={12} />Edit description</button></Show>
        }><span class="tv-doc-state"><Icon name="pencil" size={11} />Being edited below</span></Show>
      </header>
      <div class="act-body" classList={{ muted: !t().text }}>
        <Show when={t().text} fallback={<p>No description yet.</p>}><Rich text={t().text} /></Show>
        <Files files={t().files} />
      </div>
      <Show when={!t().writable}><div class="hint">Update Zerus to edit this task.</div></Show>
      <Show when={t().writable && !queued(t().stage)}><div class="hint">Description, attachments and assignment can be edited in To do or Backlog.</div></Show>
    </section>
  );
}

function TaskPage(props: { task: TaskVM }) {
  const t = () => props.task;
  const notes = () => t().discussion;
  const truncated = () => (t().logTotal ?? 0) > t().log.length;
  return (
    <>
      <Show when={(t().stage === "review" || t().stage === "done") && (t().summary || t().cancelledAt)}><ResultEntry task={t()} /></Show>
      <Description task={t()} />
      <div class="tv-section"><span class="eyebrow">Discussion</span><Show when={notes().length}><span class="count">{notes().length}</span></Show></div>
      <For each={notes()} fallback={<div class="tv-placeholder">Messages and notes appear here.</div>}>{(n) => <Note note={n} />}</For>
      <Show when={truncated()}><button class="tv-placeholder link" onClick={() => setPage("history")}>Showing recent discussion. Earlier records are available in History.</button></Show>
    </>
  );
}

function HistoryLine(props: { entry: TaskLog }) {
  const e = () => props.entry;
  const message = () => e().kind === "comment" || e().kind === "report";
  return (
    <Show when={message()} fallback={
      <div class="tv-event"><i aria-hidden="true" /><span><b>{e().actor}</b> <span class="act-time">{e().at}</span> · {historyAction(e())}</span></div>
    }>
      <article class="act-msg tv-entry tv-hmsg" classList={{ "k-user": !e().agent, "k-agent": !!e().agent }}>
        <header><b>{e().actor}</b><span class="chip">{e().kind === "report" ? "Report" : "Comment"}</span><span class="act-time">{e().at}</span></header>
        <div class="act-body"><Rich text={e().text ?? ""} /><Files files={e().files} /></div>
      </article>
    </Show>
  );
}

function HistoryPage(props: { task: TaskVM }) {
  const t = () => props.task;
  const [stale, setStale] = createSignal(false);
  // Paging back freezes the window; a newer event asks to return to Latest.
  createEffect(on(() => t().log.length, () => { if (historyOlder()) setStale(true); }, { defer: true }));
  createEffect(on(historyOlder, (older) => { if (!older) setStale(false); }));
  const source = () => (historyOlder() ? t().olderLog ?? [] : t().log);
  const first = () => Math.max(0, source().length - historyVisible());
  const more = () => first() > 0 ? `Earlier events (${first()})` : !historyOlder() && (t().logTotal ?? 0) > t().log.length ? "Earlier events…" : "";
  return (
    <>
      <div class="tv-history-nav">
        <Show when={more()}>
          <button class="btn small ghost accent" onClick={() => first() > 0 ? setHistoryVisible(historyVisible() + HISTORY_PAGE) : (setHistoryOlder(true), setHistoryVisible(HISTORY_PAGE))}>
            <Icon name="arrow-up" size={12} />{more()}
          </button>
        </Show>
        <span class="grow" />
        <Show when={historyOlder()}><button class="btn small ghost accent" onClick={() => { setHistoryOlder(false); setHistoryVisible(HISTORY_PAGE); }}>Latest<Icon name="arrow-down" size={12} /></button></Show>
      </div>
      <Show when={stale()}><div class="notice tv-line"><Icon name="info" size={13} /><span>History changed; return to Latest to refresh.</span></div></Show>
      <Show when={source().length} fallback={<div class="tv-placeholder">No history yet.</div>}>
        <div class="tv-timeline"><For each={source().slice(first())}>{(e) => <HistoryLine entry={e} />}</For></div>
      </Show>
    </>
  );
}

// Title drafts survive leaving the task (QSettings tasks/titleDraft/<project>/<uuid>).
const [titleDrafts, setTitleDrafts] = createSignal<Record<string, string>>({});

/** Click to rename; Enter or leaving saves, Escape cancels; a refusal keeps the edit. */
function Title(props: { task: TaskVM }) {
  let field!: HTMLTextAreaElement;
  const t = () => props.task;
  const draft = () => titleDrafts()[t().id] ?? "";
  const setDraft = (value: string) => {
    const next = { ...titleDrafts() };
    if (value) next[t().id] = value; else delete next[t().id];
    setTitleDrafts(next);
  };
  const [error, setError] = createSignal("");
  const begin = () => { if (!t().writable || !canLeave()) return; if (!draft()) setDraft(displayTitle(t())); setError(""); setRenaming(true); };
  createEffect(on(renaming, (on) => { if (on) { if (!draft()) setDraft(displayTitle(t())); queueMicrotask(() => { field?.focus(); field?.select(); }); } }));
  const finish = () => {
    if (!renaming()) return;
    const why = rename(t().id, draft());
    if (why) { setError(why); return; }
    setRenaming(false); setDraft(""); setError("");
  };
  const cancel = () => { setRenaming(false); setDraft(""); setError(""); };
  return (
    <div class="tv-title-col">
      <Show when={renaming()} fallback={
        <h1 class="tv-title" classList={{ editable: t().writable }} tabindex={t().writable ? 0 : -1} title={`${t().id} · ${STAGE_LABEL[t().stage]}${t().writable ? "\nClick to rename" : ""}`}
          onClick={begin} onKeyDown={(e) => { if (e.key === "Enter" || e.key === "F2") { e.preventDefault(); begin(); } }}>{displayTitle(t())}</h1>
      }>
        <textarea ref={field} class="field tv-title-edit" rows={2} value={draft()} aria-label="Task title · Enter to save, Escape to cancel" maxlength={512}
          onInput={(e) => setDraft(e.currentTarget.value)} onBlur={finish}
          onKeyDown={(e) => {
            if (e.key === "Enter") { e.preventDefault(); finish(); }
            else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); cancel(); }
          }} />
      </Show>
      <Show when={error()}><div class="tv-title-error" role="alert">{error()}</div></Show>
    </div>
  );
}

const COMMAND_LABEL: Partial<Record<Action, string>> = { accept: "Done", backToTodo: "To do", toTodo: "To do", toBacklog: "Backlog" };

function Header(props: { task: TaskVM }) {
  const t = () => props.task;
  const blocked = () => blockedOf(t());
  const primary = () => primaryOf(t().stage);
  // Workflow commands: the stage's actions without details, rename and top; the primary last.
  const commands = createMemo<Action[]>(() => {
    const list: Action[] = actionsOf(t().stage).filter((a) => a !== "edit" && a !== "rename" && a !== "top");
    return list.includes(primary()) ? list : [...list, primary()];
  });
  const reason = () => blocked()[primary()];
  const showState = () => t().stage === "active" && (!t().startConfirmed || t().target.kind === "subagent");
  const executor = () => executorSession(t());
  const run = (a: Action, el: Element) => {
    const why = blocked()[a];
    if (why) { showReason(el, why); return; }
    if (a === "cancel") { setCancelTarget(t().id); setDialog("task-cancel"); return; }
    act(t(), a);
  };
  const menu = (el: Element) => openTaskMenu(below(el), "Task actions", [
    { label: "Rename task", icon: "pencil", reason: t().writable ? undefined : "Update Zerus to edit this task", action: () => setRenaming(true) },
    { separator: true },
    { label: "Task and discussion", checked: page() === "task", action: () => setPage("task") },
    { label: "Result", checked: page() === "result", action: () => setPage("result") },
    { label: "History", checked: page() === "history", action: () => setPage("history") },
  ]);
  const stagePop = () => recentFlight()?.id === t().id ? recentFlight()!.at : 0;
  return (
    <header class="tv-head">
      <div class="tv-title-row">
        <Title task={t()} />
        <button class="icon-btn tv-more" title="Task actions" aria-label="Task actions" aria-haspopup="menu" onClick={(e) => menu(e.currentTarget)}><Icon name="ellipsis" size={17} /></button>
      </div>
      <div class="tv-context">
        <Show when={stagePop() + 1} keyed>
          <span class={`tv-stage st-${t().stage}`} classList={{ pop: stagePop() > 0 }}><TaskGlyph task={t()} />{STAGE_LABEL[t().stage]}</span>
        </Show>
        <span class="mono tv-id" title="Task ID">{t().id}</span>
        <button class="tchip tv-assign" title={queued(t().stage) ? `${assignmentText(t())}\nChange executor assignment` : `${assignmentText(t())}\nOpen executor Activity`}
          aria-label="Task executor" disabled={queued(t().stage) ? !t().writable : !executor()}
          onClick={(e) => {
            if (!queued(t().stage)) { openExecutor(t()); return; }
            const r = e.currentTarget.getBoundingClientRect();
            setAssignMenu({ x: r.left, y: r.bottom + 4, task: t().id });
          }}>
          <Icon name={queued(t().stage) ? "user-round-cog" : "bot"} size={12} />
          <span class="ell">{assignmentText(t())}</span>
          <Icon name={queued(t().stage) ? "chevron-down" : "arrow-up-right"} size={12} />
        </button>
        <button class="icon-btn small tv-icon" title="Open executor Activity" aria-label="Open executor Activity" disabled={!executor()} onClick={() => openExecutor(t())}><Icon name="external-link" size={14} /></button>
        <button class="icon-btn small tv-icon" title={describing() ? "Task description is being edited below" : "Edit description"} aria-label="Edit description"
          disabled={!(t().writable && queued(t().stage))} onClick={() => openTask(t().id, { describe: true })}><Icon name="file-text" size={14} /></button>
        <span class="grow" />
        <div class="tv-commands" role="group" aria-label="Task workflow">
          <For each={commands()}>{(a) => (
            <button class="btn small tv-cmd" classList={{ "is-primary": a === primary(), blocked: !!blocked()[a] }} aria-label={ACTION_LABEL[a]}
              title={blocked()[a] ?? ACTION_LABEL[a]} aria-disabled={!!blocked()[a]} onClick={(e) => run(a, e.currentTarget)}>
              <Icon name={ACTION_ICON[a]} size={13} />{COMMAND_LABEL[a] ?? ACTION_LABEL[a]}
            </button>
          )}</For>
        </div>
      </div>
      <Show when={showState()}><div class="tv-state"><Icon name="hourglass" size={12} />{contextLine(t())}</div></Show>
      <Show when={!showState() && t().stage !== "active"}><div class="tv-context-line">{contextLine(t())}</div></Show>
      <Show when={reason()}>
        <div class="tv-blocker" classList={{ quiet: primary() === "pause" }}>
          <Icon name={primary() === "pause" ? "info" : "triangle-alert"} size={13} />
          <span>{primary() === "sendNow" ? `Cannot start: ${reason()}` : reason()}</span>
          <span class="grow" />
          <Show when={t().launch}><button class="btn small" onClick={() => setDialog("task-launch-review")}><Icon name="eye" size={12} />Review executor launch</button></Show>
        </div>
      </Show>
    </header>
  );
}

function Empty(props: { icon: string; text: string; sub?: string; tone?: "danger" }) {
  return (
    <div class="tv-empty" classList={{ danger: props.tone === "danger" }}>
      <Icon name={props.icon} size={26} />
      <p>{props.text}</p>
      <Show when={props.sub}><p class="hint">{props.sub}</p></Show>
    </div>
  );
}

/** The central Task View: header, the selected page (task and discussion, result or history) and the task input. */
export function TaskView() {
  // Selection keeps the previous painted document until the selected reply arrives.
  const [shown, setShown] = createSignal(selectedTask());
  const [pending, setPending] = createSignal(false);
  createEffect(on(selectedTask, (id) => {
    if (id === shown()) return;
    if (!id) { setShown(undefined); return; }
    setPending(true);
    const timer = setTimeout(() => { setShown(id); setPending(false); }, 260);
    onCleanup(() => clearTimeout(timer));
  }, { defer: true }));
  const c = () => context();
  // Only the open session's project: a selection from another project is not shown.
  const task = () => { const t = taskById(shown()); return t && t.project === c().project ? t : undefined; };
  return (
    <div class="tv" style={scaleStyle()} aria-busy={pending() || busy()}>
      <Show when={pending()}><div class="tv-progress" aria-hidden="true" /></Show>
      <Switch>
        <Match when={!c().project && !task()}><Empty icon="list-todo" text="Open a project session to see tasks" /></Match>
        <Match when={taskDemo === "mismatch"}><Empty icon="circle-alert" tone="danger" text="The task reply did not match the selected task." sub="Nothing was changed; the next read replaces this view." /></Match>
        <Match when={demoState() === "error"}><Empty icon="circle-alert" tone="danger" text="Task could not be read: ssh build-01: connection timed out after 20 s" sub="The task stays as it was; reading resumes at the slow pace until a listing succeeds." /></Match>
        <Match when={demoState() === "loading" || (pending() && !task())}>
          <div class="tv-col tv-loading"><span class="skeleton" style={{ height: "20px", width: "52%" }} /><span class="skeleton" style={{ height: "14px", width: "34%" }} /><span class="skeleton" style={{ height: "90px" }} /><p class="dim">Loading task…</p></div>
        </Match>
        <Match when={!task()}><Empty icon="list-todo" text="Select a task in the Tasks panel to see its details." sub="Open the side panel's Tasks tab and click a card." /></Match>
        <Match when={task()}>{(t) => (
          <>
            <div class="tv-col"><Header task={t()} /></div>
            <div class="tv-scroll">
              <div class="tv-col tv-content" classList={{ stale: pending() }}>
                <Show when={page() !== "task"}>
                  <div class="tv-section"><span class="eyebrow">{page() === "result" ? "Result" : "History"}</span><span class="grow" /><button class="link tv-back" onClick={() => setPage("task")}><Icon name="chevron-left" size={12} />Task and discussion</button></div>
                </Show>
                <Switch>
                  <Match when={page() === "task"}><TaskPage task={t()} /></Match>
                  <Match when={page() === "result"}>
                    <Show when={t().summary || t().cancelledAt} fallback={<div class="tv-placeholder">No result reported yet.</div>}><ResultEntry task={t()} /></Show>
                  </Match>
                  <Match when={page() === "history"}><HistoryPage task={t()} /></Match>
                </Switch>
              </div>
            </div>
            <div class="cmp-col"><TaskComposer task={t()} /></div>
          </>
        )}</Match>
      </Switch>
    </div>
  );
}
