import { For, Show, createSignal } from "solid-js";
import { Icon } from "../../lib/icons";
import { activity } from "../../mock/data";
import { selected, toast, setDetailTab, contentScale, subagentOpen, setSubagentOpen } from "../../state";
import { skin } from "../../skin/runtime";
import type { ActivityItem, Session } from "../../mock/types";

// The richest synthetic transcript belongs to api-refactor; other sessions get
// a short conversation derived from their own row so every state reads true.
function activityFor(s: Session | undefined): ActivityItem[] {
  if (!s || s.key === "ws:1") return activity;
  return [
    { id: `${s.key}-u`, kind: "user", at: "Oct 27 09:12", body: `Work on ${s.name}: keep the change small and tell me before anything destructive.`, sender: "You" },
    { id: `${s.key}-t`, kind: "tools", at: "Oct 27 09:14", title: "3 tool calls", body: "Read, Grep, Bash", lines: ["Read  README.md", "Grep  \"TODO\" src", "Bash  make check"] },
    { id: `${s.key}-a`, kind: "agent", at: "Oct 27 09:31", body: `${s.summary}${/[.?!]$/.test(s.summary) ? "" : "."}\n\nI kept the change scoped to \`${s.branch ?? "main"}\` and left a note in the task.` },
  ];
}

// Activity renders journal events. In the client, markdown is parsed off the
// UI thread (Rust, pulldown-cmark) and arrives as sanitized fragments; the
// list is windowed so long transcripts mount only what is visible.

function inline(text: string) {
  // Minimal mock formatter: `code`, **bold**, links on example domains.
  const parts = text.split(/(`[^`]+`|\*\*[^*]+\*\*|https:\/\/[a-z.]+example\.com\S*)/g);
  return parts.map((p) =>
    p.startsWith("`") ? <code>{p.slice(1, -1)}</code> :
    p.startsWith("**") ? <b>{p.slice(2, -2)}</b> :
    p.startsWith("https://") ? <a href="#" onClick={(e) => e.preventDefault()} title="Opens in the browser">{p}</a> : p,
  );
}

function Body(props: { text: string }) {
  const blocks = () => props.text.split("\n\n");
  return (
    <For each={blocks()}>
      {(b) => b.startsWith("- ") ? (
        <ul class="md-list"><For each={b.split("\n")}>{(li) => <li>{inline(li.replace(/^- /, ""))}</li>}</For></ul>
      ) : <p>{inline(b)}</p>}
    </For>
  );
}

function Item(props: { item: ActivityItem }) {
  const [open, setOpen] = createSignal(false);
  const it = props.item;
  if (it.kind === "thinking") return <div class="act-thinking"><span class="eyebrow">Thinking</span><p>{it.body}</p></div>;
  if (it.kind === "report")
    return (
      <div class="act-notice" classList={{ open: open() }}>
        <div class="row"><Icon name="bot" size={13} /><b>{it.title}</b><span class="muted">from {it.sender}</span><span class="grow" /><span class="act-time">{it.at}</span></div>
        <button class="link" onClick={() => setOpen(!open())}>{open() ? "▾ Hide report" : "▸ Show report"}</button>
        <Show when={open()}><p class="act-notice-body">{it.body}</p></Show>
      </div>
    );
  if (it.kind === "voice")
    return (
      <article class="act-msg k-user voice">
        <header><b>{it.sender}</b><span class="act-time">{it.at}</span><span class="act-receipt"><Icon name="check" size={11} />Accepted by native app</span></header>
        <div class="voice-player"><button class="icon-btn small" title="Play / pause"><Icon name="play" size={13} /></button><div class="wave" aria-hidden="true">{Array.from({ length: 48 }, (_, i) => <i style={{ height: `${4 + ((i * 37) % 17)}px` }} />)}</div><span class="dim tabular">0:09</span></div>
        <button class="link" onClick={() => setOpen(!open())}>{open() ? "Hide transcript" : "Show transcript"}</button>
        <Show when={open()}><p class="act-body" style={{ "margin-top": "4px" }}>{it.body}</p></Show>
      </article>
    );
  return (
    <Show when={it.kind !== "tools" && it.kind !== "tool"} fallback={
      <div class="act-tools" classList={{ open: open() }}>
        <button class="act-tools-head" onClick={() => setOpen(!open())} aria-expanded={open()}>
          <Icon name="chevron-right" size={12} class="chev" />
          <b>{it.title}</b>
          <Show when={it.kind === "tools"}><span class="muted">/ {it.body}</span></Show>
          <span class="act-time">{it.at}</span>
        </button>
        <Show when={it.kind === "tool"}><div class="act-tool-cmd mono">{it.body}</div></Show>
        <Show when={open() && it.lines}>
          <div class="act-tool-lines mono"><For each={it.lines}>{(l) => <div>{l}</div>}</For></div>
        </Show>
      </div>
    }>
      <article class={`act-msg k-${it.kind}`}>
        <header>
          <b>{it.kind === "user" ? "You" : it.kind === "agent" ? "Agent" : it.sender ?? "Zerus"}</b>
          <span class="act-time">{it.at}</span>
          <Show when={it.kind === "user"}><span class="act-receipt"><Icon name="check" size={11} />Sent</span></Show>
          <span class="grow" />
          <Show when={it.kind === "agent"}><button class="icon-btn small act-copy" title="Copy as Markdown" onClick={() => toast("Copied as Markdown", "The reply is on the clipboard")}><Icon name="copy" size={12} /></button></Show>
        </header>
        <div class="act-body">
          <Body text={it.body ?? ""} />
          <Show when={it.code}><pre class="act-code mono">{it.code}</pre></Show>
          <Show when={it.attachments}>
            <div class="act-attachments">
              <For each={it.attachments}>{(a) => (
                <button class="att-chip" onClick={() => toast("Attachment", `Opens ${a.name} in the viewer`)}>
                  <Icon name={a.kind === "image" ? "image" : "file-text"} size={14} />
                  <span>{a.name}</span><span class="dim">{a.size}</span>
                </button>
              )}</For>
            </div>
          </Show>
        </div>
      </article>
    </Show>
  );
}

export function QuestionCard() {
  const s = () => selected();
  const [choice, setChoice] = createSignal<number | undefined>(0);
  const approval = () => s()?.attention === "approval";
  return (
    <section class="question-card" aria-label="Agent question">
      <header>
        <span class="q-title">{approval() ? "Agent asks for permission" : "Agent needs your answer"}</span>
        <span class="grow" />
        <span class="dim">{approval() ? "Bash" : "0 of 1 answered"}</span>
      </header>
      <Show when={!approval()} fallback={
        <>
          <p class="q-text">Run command?</p>
          <pre class="act-code mono">sqlx migrate run --database-url $DATABASE_URL</pre>
          <div class="q-options">
            <For each={["Yes", "Yes, and don't ask again for sqlx commands in this folder", "No, and tell the agent what to do differently"]}>
              {(label, i) => (
                <label class="q-option" classList={{ on: choice() === i() }}>
                  <input type="radio" name="q" checked={choice() === i()} onChange={() => setChoice(i())} />
                  <span>{label}</span>
                </label>
              )}
            </For>
          </div>
        </>
      }>
        <p class="q-text">Which form layout should I keep for checkout?</p>
        <div class="q-options">
          <For each={[
            { label: "Single column (Recommended)", detail: "Matches the mobile layout and keeps keyboard order simple." },
            { label: "Two columns", detail: "Street and city side by side on wide screens." },
            { label: "Other", detail: "Write your own answer" },
          ]}>
            {(o, i) => (
              <label class="q-option" classList={{ on: choice() === i() }}>
                <input type="radio" name="q" checked={choice() === i()} onChange={() => setChoice(i())} />
                <span><b>{o.label}</b><small>{o.detail}</small></span>
              </label>
            )}
          </For>
        </div>
      </Show>
      <footer>
        <button class="btn small ghost" onClick={() => setDetailTab("native")}>Open native UI</button>
        <span class="grow" />
        <span class="hint">{approval() ? "Delivery is verified on the agent's screen before it counts." : "Answer each question to continue."}</span>
        <button class="btn primary" onClick={() => toast("Answer sent", "Delivered to the agent's native prompt")}>{approval() ? "Confirm" : "Submit answer"}</button>
      </footer>
    </section>
  );
}

export function ActivityView() {
  const full = () => skin().layout["activity-column"] === "full";
  const s = () => selected();
  return (
    <div class="activity" style={{ "--content-scale": contentScale() }}>
      <div class="act-scroll">
        <div class="act-col" classList={{ full: full() }}>
          <Show when={subagentOpen()}>
            <div class="subagent-bar">
              <button class="btn small" onClick={() => setSubagentOpen(undefined)}><Icon name="arrow-right" size={12} style={{ rotate: "180deg" }} />Main activity</button>
              <b>{s()?.subagents?.find((k) => k.id === subagentOpen())?.name}</b>
              <span class="hint">Showing recent activity. Read only: this agent is managed by the parent conversation.</span>
            </div>
          </Show>
          <div class="act-label eyebrow">Recorded activity — messages and tool excerpts</div>
          <Show when={s()?.status !== "ended"} fallback={<div class="notice"><Icon name="archive" size={14} /><div>This conversation is archived. <a href="#" onClick={(e) => { e.preventDefault(); toast("Restore", "Restoring the session"); }}>Restore</a> to continue it.</div></div>}>
            <For each={activityFor(s())}>{(it) => <Item item={it} />}</For>
            <Show when={s()?.status === "working" && !subagentOpen()}>
              <div class="queued"><div class="row"><b>Queued in agent</b><span class="grow" /><button class="btn small" title="Send queued messages now (Ctrl+Shift+Return)">Send now</button></div><div class="mono dim">Also run the macOS contract suite before you finish.</div></div>
            </Show>
            <Show when={s()?.status === "working"}>
              <div class="act-live"><span class="typing"><i /><i /><i /></span>Agent is running <b class="mono">cargo test --workspace</b><span class="grow" /><span class="dim tabular">{s()?.turnStartedMin}m</span></div>
            </Show>
            <Show when={s()?.status === "attention"}><QuestionCard /></Show>
            <Show when={s()?.status === "error"}>
              <div class="notice danger"><Icon name="circle-alert" size={14} /><div><b>Provider error.</b> {s()?.errorText}. Automatic recovery will retry; you can also resume now. <button class="btn small" onClick={() => toast("Resume", "Resuming the conversation")}>Resume now</button></div></div>
            </Show>
            <Show when={s()?.status === "offline"}>
              <div class="notice warn"><Icon name="wifi-off" size={14} /><div><b>{s()?.machine} is offline.</b> This is the last activity received {s()?.lastEvent}. Messages you write stay as a draft until it reconnects.</div></div>
            </Show>
            <Show when={s()?.unread}>
              <button class="mark-read" onClick={() => toast("Marked as read", "Reply marked as read on all devices")}><Icon name="check-check" size={13} />Mark as read</button>
            </Show>
          </Show>
        </div>
      </div>
    </div>
  );
}
