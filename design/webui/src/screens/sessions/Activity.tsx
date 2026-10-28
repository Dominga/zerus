import { For, Show, createEffect, createRoot, createSignal, on, onCleanup, onMount } from "solid-js";
import { Icon } from "../../lib/icons";
import { selected, toast, setDetailTab, contentScale, subagentOpen, setSubagentOpen, setMenu, type MenuEntry } from "../../state";
import { skin } from "../../skin/runtime";
import { demoTranscript, VOICE_STATE_LABEL, voiceStateDetail, voiceStateIssue, type MediaMessage, type TranscriptItem } from "../../mock/activity-extra";
import type { Session } from "../../mock/types";

// ---------------------------------------------------------------------------
// Reading column (ActivityWidth.h): 900 px at 100% content scale, 480–2400,
// scaled with the content scale, or the full pane. The transcript, queue,
// questions, banners and the composer all share it; Terminal, Processes and
// Native UI ignore it. The skin's L2 activity-column variant is the start.

export const COLUMN = { min: 480, max: 2400, initial: 900, edge: 10 } as const;
const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
const savedColumn = (() => { try { return Number(localStorage.getItem("zerus-mock-activity-width")) || 0; } catch { return 0; } })();
const [columnWidth, setColumnWidthRaw] = createSignal(savedColumn ? clamp(savedColumn, COLUMN.min, COLUMN.max) : COLUMN.initial);
const [fullWidth, setFullWidth] = createSignal(skin().layout["activity-column"] === "full");
createRoot(() => createEffect(on(() => skin().layout["activity-column"], (v) => setFullWidth(v === "full"), { defer: true })));
export { columnWidth, fullWidth };

function setColumnWidth(width: number) {
  setColumnWidthRaw(clamp(Math.round(width), COLUMN.min, COLUMN.max));
  try { localStorage.setItem("zerus-mock-activity-width", String(columnWidth())); } catch { /* storage may be unavailable */ }
}
/** Double-click on an edge or Reset Activity width: the default, centered. */
export function resetColumn() { setColumnWidth(COLUMN.initial); setFullWidth(false); }

/** CSS variables for everything that shares the column (set on the Activity tab body). */
export const columnStyle = () => ({
  "--content-scale": String(contentScale()),
  "--col": fullWidth() ? "100%" : `${Math.round(columnWidth() * contentScale())}px`,
});

/** Full-width Activity (checkable) and Reset Activity width (disabled at the default). */
export function columnMenuItems(): MenuEntry[] {
  const atDefault = !fullWidth() && columnWidth() === COLUMN.initial;
  return [
    { label: "Full-width Activity", checked: fullWidth(), action: () => setFullWidth(!fullWidth()) },
    { label: "Reset Activity width", icon: "rotate-ccw", disabled: atDefault, action: atDefault ? undefined : resetColumn },
  ];
}

/** A 10 px column edge in the transcript margin; both edges move together. */
function ColumnEdge(props: { right?: boolean }) {
  const [dragging, setDragging] = createSignal(false);
  let origin = 0, start = 0, widest = 0, narrowest = 0;
  const measure = (el: HTMLElement) => {
    const scroller = el.closest(".act-scroll") as HTMLElement;
    start = el.parentElement!.getBoundingClientRect().width;
    // Its edges stay within the pane; the width keeps a readable minimum.
    widest = scroller.clientWidth - 2 * COLUMN.edge;
    narrowest = Math.min(widest, COLUMN.min * contentScale());
  };
  const apply = (distance: number) => {
    setColumnWidth(clamp(start + 2 * distance, narrowest, widest) / contentScale());
    setFullWidth(false);
  };
  return (
    <div class="col-edge" classList={{ right: props.right, dragging: dragging() }} role="separator" aria-orientation="vertical" tabindex="0"
      aria-label="Activity width" aria-valuemin={COLUMN.min} aria-valuemax={COLUMN.max} aria-valuenow={columnWidth()}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        const el = e.currentTarget;
        el.setPointerCapture(e.pointerId);
        origin = e.clientX;
        measure(el);
        setDragging(true);
      }}
      onPointerMove={(e) => { if (dragging()) apply(props.right ? e.clientX - origin : origin - e.clientX); }}
      onPointerUp={(e) => { setDragging(false); e.currentTarget.releasePointerCapture(e.pointerId); }}
      onPointerCancel={() => setDragging(false)}
      onDblClick={resetColumn}
      onKeyDown={(e) => {
        const step = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0;
        if (!step) return;
        e.preventDefault();
        measure(e.currentTarget);
        apply((props.right ? step : -step) * 20);
      }}
      onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); setMenu({ x: e.clientX, y: e.clientY, items: columnMenuItems() }); }}
    />
  );
}

/** The text browser's menu, then the two width items. */
function activityMenu(e: MouseEvent) {
  if ((e.target as HTMLElement).closest("button, a, input, textarea, .col-edge")) return;
  e.preventDefault();
  const scroller = e.currentTarget as HTMLElement;
  const text = getSelection()?.toString() ?? "";
  setMenu({
    x: e.clientX, y: e.clientY, items: [
      { label: "Copy", icon: "copy", kbd: "Ctrl+C", disabled: !text, action: text ? () => { void navigator.clipboard?.writeText(text).catch(() => {}); toast("Copied", "The selected text is on the clipboard"); } : undefined },
      { label: "Select All", kbd: "Ctrl+A", action: () => { const col = scroller.querySelector(".act-col"); if (col) getSelection()?.selectAllChildren(col); } },
      { separator: true },
      ...columnMenuItems(),
    ],
  });
}

// ---------------------------------------------------------------------------
// Transcript

// The richest synthetic transcript belongs to api-refactor; other sessions get
// a short conversation derived from their own row so every state reads true.
function activityFor(s: Session | undefined): TranscriptItem[] {
  if (!s || s.key === "ws:1") return demoTranscript;
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

// ---------------------------------------------------------------------------
// Voice and round-video messages (ActivityView.cpp:978-1025, VoicePlayerImage.h)

const mediaTime = (ms: number) => {
  const s = Math.floor(Math.max(0, ms) / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

// One recording plays at a time, as with the view's single media player.
const [playing, setPlaying] = createSignal<string>();

/** Delivery state glyph (18 px); media state never claims reading or completion. */
function DeliveryGlyph(props: { item: MediaMessage }) {
  const s = props.item.state;
  return (
    <span class="vm-state" classList={{ issue: voiceStateIssue(s) }} title={voiceStateDetail(s)} role="img" aria-label={VOICE_STATE_LABEL[s]}>
      <svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <Show when={s === "submitted"} fallback={
          <Show when={s === "cancelled"} fallback={
            <>
              <circle cx="9" cy="9" r="6.5" />
              <Show when={voiceStateIssue(s)} fallback={<path d="M9 5V9L12 10" />}><path d="M9 5V10M9 13H9.01" /></Show>
            </>
          }><path d="M5 5L13 13M13 5L5 13" /></Show>
        }><path d="M3 9L7 13L15 5" /></Show>
      </svg>
    </span>
  );
}

/** 32×32 transcript toggle: a down arrow beside an "A", or a chevron when open. */
function TranscriptButton(props: { open: boolean; onToggle: () => void }) {
  return (
    <button class="vm-tbtn" data-tip={props.open ? "Hide transcript" : "Show transcript"} aria-label={props.open ? "Hide transcript" : "Show transcript"}
      aria-expanded={props.open} onClick={props.onToggle}>
      <svg width="32" height="32" viewBox="0 0 32 32" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <Show when={props.open} fallback={<path d="M10 8V19M6 15L10 19L14 15M16 24L20.5 12L25 24M17.5 20H23.5" />}><path d="M10 19L16 13L22 19" /></Show>
      </svg>
    </button>
  );
}

function Waveform(props: { values: number[] }) {
  const bars = () => (props.values.length === 64 ? props.values : []);
  const layer = (cls: string) => (
    <span class={`vp-bars ${cls}`}>
      <Show when={bars().length} fallback={<i class="flat" />}>
        <For each={bars()}>{(v) => <i style={{ height: `${Math.max(2, (27 * v) / 255)}px` }} />}</For>
      </Show>
    </span>
  );
  return <>{layer("rest")}{layer("played")}</>;
}

function MediaCard(props: { item: MediaMessage }) {
  const it = props.item;
  const [transcript, setTranscript] = createSignal(false);
  const [position, setPosition] = createSignal(0);
  let timer: number | undefined;
  const stop = () => { clearInterval(timer); timer = undefined; if (playing() === it.id) setPlaying(undefined); };
  const play = () => {
    if (position() >= it.durationMs) setPosition(0);
    setPlaying(it.id);
    clearInterval(timer);
    timer = window.setInterval(() => {
      const next = position() + 100;
      if (next >= it.durationMs) { setPosition(it.durationMs); stop(); } else setPosition(next);
    }, 100);
  };
  createEffect(() => { if (playing() !== it.id && timer) { clearInterval(timer); timer = undefined; } });
  onCleanup(stop);
  const isPlaying = () => playing() === it.id;
  const seek = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setPosition(Math.round(clamp((e.clientX - r.left) / r.width, 0, 1) * it.durationMs));
  };
  const video = it.media === "video";
  return (
    <article class="act-msg k-user media" id={`item-${it.id}`}>
      <header>
        <b>{video ? "You (video)" : "You (voice)"}</b>
        <span class="grow" />
        <span class="act-time">{it.at}</span>
        <DeliveryGlyph item={it} />
      </header>
      <Show when={video} fallback={
        <div class="vm-row">
          <div class="vp" style={{ "--p": String(position() / it.durationMs) }}>
            <button class="vp-btn" aria-label={`Play or pause voice message (${mediaTime(it.durationMs)})`} aria-pressed={isPlaying()}
              onClick={() => (isPlaying() ? stop() : play())}>
              <svg width="44" height="44" viewBox="0 0 44 44" aria-hidden="true">
                <Show when={isPlaying()} fallback={<path d="M17 12L31 22L17 32Z" />}>
                  <rect x="15" y="13" width="5" height="18" rx="1" /><rect x="24" y="13" width="5" height="18" rx="1" />
                </Show>
              </svg>
            </button>
            <div class="vp-side">
              <div class="vp-wave" role="slider" tabindex="-1" aria-label="Seek" aria-valuemin={0} aria-valuemax={it.durationMs} aria-valuenow={position()} onClick={seek}>
                <Waveform values={it.waveform} />
              </div>
              <span class="vp-caption tabular">{mediaTime(position())} / {mediaTime(it.durationMs)}</span>
            </div>
          </div>
          <Show when={it.transcript}><TranscriptButton open={transcript()} onToggle={() => setTranscript(!transcript())} /></Show>
        </div>
      }>
        {/* The round video plays in place: click plays or pauses; a ring shows progress. */}
        <div class="vm-row video">
          <button class="vm-poster" classList={{ playing: isPlaying(), started: position() > 0 && position() < it.durationMs }}
            style={{ "--p": String(position() / it.durationMs) }} aria-pressed={isPlaying()}
            aria-label={`${isPlaying() ? "Pause" : "Play"} video message (${mediaTime(it.durationMs)})`} onClick={() => (isPlaying() ? stop() : play())}>
            <span class="vm-frames" aria-hidden="true" />
            <svg class="vm-ring" viewBox="0 0 200 200" aria-hidden="true">
              <circle class="vm-ring-track" cx="100" cy="100" r="97" pathLength="100" />
              <circle class="vm-ring-done" cx="100" cy="100" r="97" pathLength="100" style={{ "stroke-dashoffset": String(100 - (100 * position()) / it.durationMs) }} />
            </svg>
            <span class="vm-play" aria-hidden="true">
              <svg width="48" height="48" viewBox="0 0 48 48">
                <Show when={isPlaying()} fallback={<path d="M18 12L36 24L18 36Z" />}><rect x="15" y="13" width="6" height="22" rx="1.5" /><rect x="27" y="13" width="6" height="22" rx="1.5" /></Show>
              </svg>
            </span>
            <span class="vm-time tabular">{position() > 0 ? `${mediaTime(position())} / ` : ""}{mediaTime(it.durationMs)}</span>
          </button>
          <Show when={it.transcript}><TranscriptButton open={transcript()} onToggle={() => setTranscript(!transcript())} /></Show>
        </div>
      </Show>
      <Show when={transcript() && it.transcript}><p class="vm-text">{it.transcript}</p></Show>
      <Show when={it.error}><p class="vm-error">{it.error}</p></Show>
    </article>
  );
}

function Item(props: { item: TranscriptItem }) {
  const [open, setOpen] = createSignal(false);
  const it = props.item;
  if (it.kind === "media") return <MediaCard item={it} />;
  if (it.kind === "thinking") return <div class="act-thinking" id={`item-${it.id}`}><span class="eyebrow">Thinking</span><p>{it.body}</p></div>;
  if (it.kind === "report")
    return (
      <div class="act-notice" id={`item-${it.id}`} classList={{ open: open() }}>
        <div class="row"><Icon name="bot" size={13} /><b>{it.title}</b><span class="muted">from {it.sender}</span><span class="grow" /><span class="act-time">{it.at}</span></div>
        <button class="link" onClick={() => setOpen(!open())}>{open() ? "▾ Hide report" : "▸ Show report"}</button>
        <Show when={open()}><p class="act-notice-body">{it.body}</p></Show>
      </div>
    );
  return (
    <Show when={it.kind !== "tools" && it.kind !== "tool"} fallback={
      <div class="act-tools" id={`item-${it.id}`} classList={{ open: open() }}>
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
      <article class={`act-msg k-${it.kind}`} id={`item-${it.id}`}>
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
  const s = () => selected();
  let scroller!: HTMLDivElement;
  // Review links may name an item: app.html?…#item-a3c opens Activity scrolled to it.
  onMount(() => {
    const id = location.hash.slice(1);
    if (id.startsWith("item-")) requestAnimationFrame(() => scroller.querySelector(`#${CSS.escape(id)}`)?.scrollIntoView({ block: "start", behavior: "instant" }));
  });
  return (
    <div class="activity">
      <div class="act-scroll" ref={scroller} onContextMenu={activityMenu}>
        <div class="act-col">
          <Show when={!fullWidth()}><ColumnEdge /><ColumnEdge right /></Show>
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
          </Show>
        </div>
      </div>
    </div>
  );
}
