import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from "solid-js";
import { attachmentOpen } from "../../bridge";
import { notify } from "../../store/actions";
import { previews } from "../../store/attachments";
import { cardTarget, createOpen } from "./attachment-card";
import { createStore, reconcile } from "solid-js/store";
import { Icon } from "../lib/icons";
import { selected, toast, contentScale, subagentOpen, setSubagentOpen, setMenu, type MenuEntry } from "../../store/ui";
import { activity } from "../../store/session";
import { flag, num, setSetting } from "../../store/settings";
import type { ActivitySource, AttachmentScope } from "../../store/activitySource";
import { buildTranscript, VOICE_STATE_LABEL, voiceStateDetail, voiceStateIssue } from "./transcript";
import { statusOf } from "./SessionCard";
import type { ItemAttachment, MediaMessage, Session, TranscriptItem } from "./types";

// ---------------------------------------------------------------------------
// Reading column (ActivityWidth.h): 900 px at 100% content scale, 480–2400,
// scaled with the content scale, or the full pane. The transcript, queue,
// questions, banners and the composer all share it; Terminal, Processes and
// Native UI ignore it.

export const COLUMN = { min: 480, max: 2400, initial: 900, edge: 10 } as const;
const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
// The width is Settings > Appearance > Activity width (workspace/activityWidth,
// workspace/activityFullWidth): a drag shows it live and stores it once, when
// the pointer lets go. Choosing a skin applies its activity-column variant
// (skin/runtime.ts chooseSkin).
const [dragColumn, setDragColumn] = createSignal<number | undefined>();
const columnWidth = () => dragColumn() ?? clamp(Math.round(num("workspace/activityWidth")), COLUMN.min, COLUMN.max);
const fullWidth = () => flag("workspace/activityFullWidth");
const setFullWidth = (full: boolean) => { if (full !== fullWidth()) void setSetting("workspace/activityFullWidth", full); };
export { columnWidth, fullWidth };

/** Stores a width once, and only when it differs from the stored one. */
function storeColumn(width: number) {
  setDragColumn(undefined);
  const next = clamp(Math.round(width), COLUMN.min, COLUMN.max);
  if (next !== clamp(Math.round(num("workspace/activityWidth")), COLUMN.min, COLUMN.max)) void setSetting("workspace/activityWidth", next);
}
/** Shows a width while Settings' slider moves, or the stored one again (undefined). */
export function previewColumn(width: number | undefined) {
  setDragColumn(width === undefined ? undefined : clamp(Math.round(width), COLUMN.min, COLUMN.max));
}
/** Double-click on an edge or Reset Activity width: the default, centered. */
export function resetColumn() { storeColumn(COLUMN.initial); setFullWidth(false); }

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
  // A drag cut off (capture lost, the edge unmounted) leaves the stored width.
  const endDrag = () => { if (dragging()) { setDragging(false); setDragColumn(undefined); } };
  onCleanup(endDrag);
  let origin = 0, start = 0, widest = 0, narrowest = 0;
  const measure = (el: HTMLElement) => {
    const scroller = el.closest(".act-scroll") as HTMLElement;
    start = el.parentElement!.getBoundingClientRect().width;
    // Its edges stay within the pane; the width keeps a readable minimum.
    widest = scroller.clientWidth - 2 * COLUMN.edge;
    narrowest = Math.min(widest, COLUMN.min * contentScale());
  };
  const width = (distance: number) => clamp(start + 2 * distance, narrowest, widest) / contentScale();
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
      onPointerMove={(e) => { if (dragging()) setDragColumn(clamp(Math.round(width(props.right ? e.clientX - origin : origin - e.clientX)), COLUMN.min, COLUMN.max)); }}
      onPointerUp={(e) => {
        e.currentTarget.releasePointerCapture(e.pointerId);
        if (!dragging()) return;
        setDragging(false);
        // A click without movement stores nothing.
        const moved = dragColumn();
        if (moved === undefined) return;
        storeColumn(moved);
        setFullWidth(false);
      }}
      onPointerCancel={endDrag}
      onLostPointerCapture={endDrag}
      onDblClick={resetColumn}
      onKeyDown={(e) => {
        const step = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0;
        if (!step) return;
        e.preventDefault();
        measure(e.currentTarget);
        storeColumn(width((props.right ? step : -step) * 20));
        setFullWidth(false);
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
      { label: "Copy", icon: "copy", kbd: "Ctrl+C", disabled: !text, action: text ? () => copyText(text, "The selected text is on the clipboard") : undefined },
      { label: "Select All", kbd: "Ctrl+A", action: () => { const col = scroller.querySelector(".act-col"); if (col) getSelection()?.selectAllChildren(col); } },
      { separator: true },
      ...columnMenuItems(),
    ],
  });
}

// ---------------------------------------------------------------------------
// Transcript

const copyText = (text: string, what: string) => {
  void navigator.clipboard?.writeText(text).then(() => toast("Copied", what), () => toast("Not copied", "The clipboard is unavailable"));
};

// Markdown is rendered in Rust later (pulldown-cmark, sanitized); until then
// text is shown as text: paragraphs, lists, fenced code, `code` and **bold**.
// Nothing is inserted as HTML, and only example-domain links are drawn as
// links (they never navigate the app).

function inline(text: string) {
  // Inline formatting: `code`, **bold**, links on example domains.
  const parts = text.split(/(`[^`]+`|\*\*[^*]+\*\*|https:\/\/[a-z.]+example\.com\S*)/g);
  return parts.map((p) =>
    p.startsWith("`") ? <code>{p.slice(1, -1)}</code> :
    p.startsWith("**") ? <b>{p.slice(2, -2)}</b> :
    p.startsWith("https://") ? <a href="#" onClick={(e) => e.preventDefault()} title={p}>{p}</a> : p,
  );
}

type Block = { kind: "p" | "ul" | "code"; text: string };

/** Splits text into paragraphs, "- " lists and ``` fenced code blocks. */
function blocks(text: string): Block[] {
  const out: Block[] = [];
  const parts = text.split(/^```[^\n]*\n?/m);
  parts.forEach((part, i) => {
    if (i % 2 === 1) { out.push({ kind: "code", text: part.replace(/\n$/, "") }); return; }
    for (const b of part.split(/\n{2,}/)) {
      const trimmed = b.replace(/^\n+|\n+$/g, "");
      if (trimmed) out.push({ kind: trimmed.startsWith("- ") ? "ul" : "p", text: trimmed });
    }
  });
  return out;
}

function Body(props: { text: string }) {
  return (
    <For each={blocks(props.text)}>
      {(b) => b.kind === "code" ? <pre class="act-code mono">{b.text}</pre> : b.kind === "ul" ? (
        <ul class="md-list"><For each={b.text.split("\n")}>{(li) => <li>{inline(li.replace(/^- /, ""))}</li>}</For></ul>
      ) : <p>{inline(b.text)}</p>}
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

const openAttachment = createOpen({
  open: (computer, file) => attachmentOpen(computer, file),
  notify: (tone, text, sticky, key) => { notify(tone, text, sticky, key); },
});

/**
 * A message's file as Qt's card shows it (ActivityView.cpp:759-782): an
 * image's thumbnail, the caption, its size and Open →. A file the message
 * saved no reference to only names itself.
 */
function AttachmentCard(props: { file: ItemAttachment; scope?: AttachmentScope }) {
  const target = createMemo(() => cardTarget(props.file, props.scope), undefined, { equals: (a, b) => JSON.stringify(a) === JSON.stringify(b) });
  // An image's thumbnail reads its file while the card shows (ActivityView.cpp:2126-2131).
  createEffect(on(target, (now) => {
    if (!now || !props.file.image) return;
    previews.show(now.computer, now.ref);
    onCleanup(() => previews.hide(now.computer, now.ref));
  }));
  const thumbnail = () => {
    const now = target();
    if (!now || !props.file.image) return undefined;
    const state = previews.state(now.computer, now.ref);
    return state.kind === "ready" && state.fetched.thumbnail ? state.url : undefined;
  };
  return (
    <Show when={target()} fallback={<span class="att-card">Attachment: {props.file.name}</span>}>
      {(now) => (
        <button class="att-card" title={props.file.caption} onClick={() => void openAttachment(now().computer, now().ref)}>
          <Show when={props.file.image}>
            <span class="att-thumb">
              <Show when={thumbnail()}>
                {(url) => <img src={url()} alt="" onError={() => previews.failed(now().computer, now().ref)} />}
              </Show>
            </span>
          </Show>
          <span class="att-caption">{props.file.caption}</span>
          <span class="dim">{props.file.meta}</span>
          <span class="att-open">Open →</span>
        </button>
      )}
    </Show>
  );
}

function Item(props: { item: TranscriptItem; scope?: AttachmentScope }) {
  const [open, setOpen] = createSignal(false);
  const it = props.item;
  if (it.kind === "media") return <MediaCard item={it} />;
  if (it.kind === "context") return <div class="act-label eyebrow" id={`item-${it.id}`}>{it.body}</div>;
  if (it.kind === "thinking") return <div class="act-thinking" id={`item-${it.id}`}><span class="eyebrow">Thinking</span><p>{it.body}</p></div>;
  if (it.kind === "report")
    return (
      <div class="act-notice" id={`item-${it.id}`} classList={{ open: open() }}>
        <div class="row"><Icon name="bot" size={13} /><b>{it.title}</b><Show when={it.sender}><span class="muted">from {it.sender}</span></Show><span class="grow" /><span class="act-time">{it.at}</span></div>
        <button class="link" onClick={() => setOpen(!open())}>{open() ? "▾ Hide report" : "▸ Show report"}</button>
        <Show when={open()}><p class="act-notice-body">{it.body}</p></Show>
      </div>
    );
  return (
    <Show when={it.kind !== "tools" && it.kind !== "tool"} fallback={
      <div class="act-tools" id={`item-${it.id}`} classList={{ open: open(), warn: !!it.warn }}>
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
          <b>{it.label ?? (it.kind === "user" ? "You" : it.kind === "agent" ? "Agent" : it.sender ?? "Zerus")}</b>
          <span class="act-time">{it.at}</span>
          <Show when={it.kind === "user" && it.receipt}><span class="act-receipt"><Icon name="check" size={11} />{it.receipt}</span></Show>
          <span class="grow" />
          <Show when={it.kind === "agent"}><button class="icon-btn small act-copy" title="Copy as Markdown" onClick={() => copyText(it.body ?? "", "The reply is on the clipboard")}><Icon name="copy" size={12} /></button></Show>
        </header>
        <div class="act-body">
          <Show when={it.kind === "user"} fallback={<Body text={it.body ?? ""} />}><p>{it.body}</p></Show>
          <Show when={it.code}><pre class="act-code mono">{it.code}</pre></Show>
          <Show when={it.attachments}>
            <div class="act-attachments">
              <For each={it.attachments}>{(a) => <AttachmentCard file={a} scope={props.scope} />}</For>
            </div>
          </Show>
        </div>
      </article>
    </Show>
  );
}

/** Within this distance of the top, scrolling reads the previous page. */
const OLDER_EDGE = 120;
/** Within this distance of the bottom, new events keep the view at the bottom. */
const STICK_EDGE = 48;

// The jump to the latest activity (ActivityView.cpp 1607-1636): whether the
// view follows the newest item, how many events arrived since it stopped,
// and the jump back. The composer toolbar shows the jump while the view does
// not follow (Qt's useCompactJumpButton in its Latest slot).
const [following, setFollowing] = createSignal(true);
const [unseen, setUnseen] = createSignal(0);
/** The mounted transcript's scroller (one Activity view shows at a time). */
let liveScroller: HTMLDivElement | undefined;
export { following, unseen };

/**
 * Puts the focus in the transcript (Qt's browser), e.g. before a focused chip
 * hides; false when no transcript is shown to take it.
 */
export function focusTranscript(): boolean {
  if (!liveScroller?.isConnected) return false;
  liveScroller.focus({ preventScroll: true });
  return true;
}

/** Scrolls to the newest item and follows it again. */
export function jumpToLatest() {
  setFollowing(true);
  setUnseen(0);
  liveScroller?.scrollTo({ top: liveScroller.scrollHeight, behavior: "instant" });
}

/** The transcript items of a source. */
export const transcriptOf = (source: ActivitySource) => buildTranscript(source.events(), source.summary());

/** Scrolling near the top reads the previous page, unless one is loading or none is left. */
export const readsOlder = (source: ActivitySource) => !source.loadingOlder() && !source.olderDone() && source.events().length > 0;

/**
 * What the pane shows besides the transcript, for its source and the selected
 * row. A read-only source (an archived row's recorded Activity) shows its
 * transcript and none of the live states; a source that lacks something shows
 * nothing for it.
 */
export function paneStates(source: ActivitySource, s: Session | undefined) {
  const live = source.readOnly || !s ? undefined : statusOf(s);
  return {
    /** "This conversation is archived." in place of the transcript. */
    archived: live === "ended",
    working: live === "working",
    attention: live === "attention",
    error: live === "error",
    offline: live === "offline",
    /** "This session left the list; its Activity stopped here." */
    ended: source.ended(),
    /** Why the last read failed, as a notice: read-only sources only (the live pane never showed its follow error). */
    failure: source.readOnly ? source.error() ?? "" : "",
  };
}

export function ActivityView(props: { source?: ActivitySource }) {
  const s = () => selected();
  /** The live follow unless the detail passes another source (an archived row's). */
  const source = (): ActivitySource => props.source ?? activity;
  let scroller!: HTMLDivElement;
  // Items keep their identity while events append: the list reconciles by id,
  // so a growing tool group updates in place and older pages only add nodes.
  const [view, setView] = createStore<{ items: TranscriptItem[] }>({ items: [] });
  const built = createMemo(() => transcriptOf(source()));
  createEffect(() => setView("items", reconcile(built(), { key: "id" })));

  // Stay at the bottom while it is in view; keep the reading position when an older page lands above.
  let anchor: { height: number; top: number } | undefined;
  /** The newest event's seq the view has seen, for the count of new ones. */
  let newest: number | undefined;
  onMount(() => { liveScroller = scroller; });
  onCleanup(() => { if (liveScroller === scroller) liveScroller = undefined; });
  const onScroll = () => {
    const atBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < STICK_EDGE;
    if (atBottom !== following()) setFollowing(atBottom);
    if (atBottom && unseen()) setUnseen(0);
    if (scroller.scrollTop < OLDER_EDGE && readsOlder(source())) {
      anchor = { height: scroller.scrollHeight, top: scroller.scrollTop };
      source().loadOlder();
    }
  };
  createEffect(on(() => source().target()?.id, () => { setFollowing(true); setUnseen(0); anchor = undefined; newest = undefined; }));
  // Events after the newest one seen count as new while the view does not follow (Qt's m_unseen).
  createEffect(on(() => source().events(), (events) => {
    const last = events.at(-1)?.seq;
    if (newest !== undefined && last !== undefined && last > newest && !following()) {
      let added = 0;
      for (let i = events.length - 1; i >= 0 && (events[i].seq ?? -Infinity) > newest; i--) added++;
      setUnseen((n) => n + added);
    }
    if (last !== undefined) newest = last;
  }));
  createEffect(on(() => view.items.length, () => {
    queueMicrotask(() => {
      if (!scroller) return;
      if (anchor && !following()) {
        const grown = scroller.scrollHeight - anchor.height;
        if (grown > 0) scroller.scrollTo({ top: anchor.top + grown, behavior: "instant" });
        anchor = undefined;
      } else if (following()) scroller.scrollTo({ top: scroller.scrollHeight, behavior: "instant" });
    });
  }));

  const summary = () => source().summary();
  const scope = createMemo(() => source().attachmentScope?.(), undefined, { equals: (a, b) => JSON.stringify(a) === JSON.stringify(b) });
  const liveTool = () => String(summary()?.current_tool || summary()?.activity_summary || s()?.summary || "");
  const states = createMemo(() => paneStates(source(), s()));
  return (
    <div class="activity">
      <div class="act-scroll" ref={scroller} tabindex="-1" onContextMenu={activityMenu} onScroll={onScroll}>
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
          <Show when={source().loadingOlder()}><div class="act-older hint">Reading earlier activity…</div></Show>
          <Show when={source().olderDone() && source().events().length > 0 && summary()?.history_truncated === true}>
            <div class="act-older hint">Older events have expired from the journal.</div>
          </Show>
          <Show when={!states().archived} fallback={<div class="notice"><Icon name="archive" size={14} /><div>This conversation is archived.</div></div>}>
            <Show when={source().loaded() || view.items.length} fallback={<div class="act-older hint">Reading activity…</div>}>
              <Show when={view.items.length} fallback={<div class="act-older hint">No recorded activity yet.</div>}>
                <For each={view.items}>{(it) => <Item item={it} scope={scope()} />}</For>
              </Show>
            </Show>
            <Show when={states().working && liveTool()}>
              <div class="act-live"><span class="typing"><i /><i /><i /></span>Agent is running <b class="mono ell">{liveTool()}</b></div>
            </Show>
            <Show when={states().attention}>
              <div class="notice warn"><Icon name="circle-alert" size={14} /><div><b>{s()?.caption}.</b> Answer it in the agent's terminal or in the Zerus app; answering here arrives with the session actions.</div></div>
            </Show>
            <Show when={states().error}>
              <div class="notice danger"><Icon name="circle-alert" size={14} /><div><b>{s()?.caption}.</b> {s()?.summary}</div></div>
            </Show>
            <Show when={states().offline}>
              <div class="notice warn"><Icon name="wifi-off" size={14} /><div><b>{s()?.machine} is offline.</b> This is the last activity received. It updates again when the machine reconnects.</div></div>
            </Show>
            <Show when={states().ended}>
              <div class="notice"><Icon name="archive" size={14} /><div>This session left the list; its Activity stopped here.</div></div>
            </Show>
            <Show when={states().failure}>
              <div class="notice danger"><Icon name="circle-alert" size={14} /><div>{states().failure}</div></div>
            </Show>
          </Show>
        </div>
      </div>
    </div>
  );
}
