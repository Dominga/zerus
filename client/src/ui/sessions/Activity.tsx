import { createActivityScroll } from "./activity-scroll";
import { activityReadingRows, createActivityAnchor } from "./activity-anchor";
import { searchPreview, clearSearchPreview } from "../../store/searchSelection";
import { SearchHighlight } from "./SearchHighlight";
import { For, Show, batch, createContext, createEffect, createMemo, createSignal, on, onCleanup, onMount, useContext } from "solid-js";
import { attachmentFetch, attachmentMedia, attachmentOpen, copyVerbatim } from "../../bridge";
import { notify } from "../../store/actions";
import { previews } from "../../store/attachments";
import { mediaPlayback } from "../../store/mediaPlayback";
import { pageHidden } from "../../store/visibility";
import { recordingBlob } from "./voice-playback";
import { cardTarget, createOpen, readCards } from "./attachment-card";
import { createStore, reconcile } from "solid-js/store";
import { Icon } from "../lib/icons";
import { selected, toast, contentScale, subagentOpen, setSubagentOpen, setMenu, setDetailTab, type MenuEntry } from "../../store/ui";
import { activity } from "../../store/session";
import { orderedRows } from "../../store/catalog";
import { flag, num, setSetting } from "../../store/settings";
import type { ActivitySource, AttachmentScope } from "../../store/activitySource";
import { composer, draftKey } from "../../store/composer";
import { terminal } from "../../store/terminal";
import type { LocalMessage } from "../../types";
import { animationFrames, createActivityEnd, newLocalSends } from "./activity-end";
import { buildTranscript, createTranscriptProjection, VOICE_STATE_LABEL, voiceStateDetail, voiceStateIssue } from "./transcript";
import { DELETE_TEXT, DELETE_TITLE, retryReason, routeInspect } from "./local-actions";
import { inlineParts, markdownBlocks } from "./markdown";
import { openExternal, openSessionFile } from "./sessionFiles";
import { SessionFileDialog } from "./SessionFileDialog";
import { statusOf } from "./SessionCard";
import type { ActivityItem, ItemAttachment, LocalItem, MediaMessage, Session, TranscriptItem } from "./types";

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
  void copyVerbatim(text).then(() => toast("Copied", what), () => toast("Not copied", "The clipboard is unavailable", "error"));
};

// Inline Markdown stays text except its links: HTTP(S) uses the system
// opener, and local/file references open the session's File reference dialog.

const ActivitySession = createContext<() => Session | undefined>();
const ActivityLayoutHeld = createContext<() => boolean>(() => false);

/** Keep disclosure geometry while a gesture or persistent selection owns
 * layout. In particular, shrinking offscreen content would clamp WebKit's
 * scrollTop even without an application scroll write (zerus-5wmh.85). */
function layoutDisclosure() {
  const held = useContext(ActivityLayoutHeld);
  const [wanted, setWanted] = createSignal(false);
  const open = createMemo((previous: boolean) => held() ? previous : wanted(), false);
  return [open, () => setWanted((value) => !value)] as const;
}

function inline(text: string, session?: () => Session | undefined) {
  return inlineParts(text).map((part) => {
    if (part.kind === "code") return <code>{part.text}</code>;
    if (part.kind === "bold") return <b>{part.text}</b>;
    if (part.kind === "external" || part.kind === "file") {
      return <a href={part.href} title={part.href} draggable={false} onAuxClick={(event) => event.preventDefault()} onDragStart={(event) => event.preventDefault()} onClick={(event) => {
        event.preventDefault();
        if (part.kind === "external") openExternal(part.href);
        else {
          const current = session?.();
          if (current) openSessionFile(current, part.href);
        }
      }}>{part.text}</a>;
    }
    return part.text;
  });
}

export function Body(props: { text: string }) {
  const session = useContext(ActivitySession);
  return (
    <For each={markdownBlocks(props.text)}>
      {(b) => {
        if (b.kind === "code") return <pre class="act-code mono">{b.text}</pre>;
        if (b.kind === "table") return (
          <div class="act-table-wrap" role="region" aria-label="Message table" tabindex="0">
            <table class="act-table" style={{ "--table-columns": b.header.length }}>
              <thead><tr><For each={b.header}>{(cell, column) => <th scope="col" style={{ "text-align": b.align[column()] }}>{inline(cell, session)}</th>}</For></tr></thead>
              <Show when={b.rows.length}><tbody><For each={b.rows}>{(row) => <tr><For each={row}>{(cell, column) => <td style={{ "text-align": b.align[column()] }}>{inline(cell, session)}</td>}</For></tr>}</For></tbody></Show>
            </table>
          </div>
        );
        if (b.kind === "ul") return <ul class="md-list"><For each={b.text.split("\n")}>{(li) => <li>{inline(li.replace(/^- /, ""), session)}</li>}</For></ul>;
        return <p>{inline(b.text, session)}</p>;
      }}
    </For>
  );
}

// ---------------------------------------------------------------------------
// Voice and round-video messages (ActivityView.cpp:978-1025, VoicePlayerImage.h)

const mediaTime = (ms: number) => {
  const s = Math.floor(Math.max(0, ms) / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

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

function MediaCard(props: { item: MediaMessage; scope?: AttachmentScope }) {
  const it = props.item;
  const video = it.media === "video";
  const [transcript, toggleTranscript] = layoutDisclosure();
  const held = useContext(ActivityLayoutHeld);
  const [position, setPosition] = createSignal(0);
  const [duration, setDuration] = createSignal(it.durationMs);
  const [isPlaying, setPlaying] = createSignal(false);
  const [loading, setLoading] = createSignal(false);
  const [url, setUrl] = createSignal("");
  const [error, setError] = createSignal("");
  const visibleError = createMemo((previous: string) => held() ? previous : error() || it.error || "", "");
  const target = createMemo(() => it.file ? cardTarget(it.file, props.scope) : undefined,
    undefined, { equals: (a, b) => JSON.stringify(a) === JSON.stringify(b) });
  let player: HTMLMediaElement | undefined, objectUrl = "";
  let claim: ReturnType<typeof mediaPlayback.take> | undefined;
  let pendingSeek: number | undefined;
  const clear = () => {
    claim?.release(); claim = undefined;
    player?.pause(); setPlaying(false); setLoading(false);
    if (player) { player.removeAttribute("src"); player.load(); }
    if (objectUrl) URL.revokeObjectURL(objectUrl);
    objectUrl = ""; pendingSeek = undefined; setUrl(""); setPosition(0);
  };
  createEffect(on(target, () => { clear(); setPosition(0); setDuration(it.durationMs); setError(""); }));
  createEffect(() => { if (pageHidden()) clear(); });
  onCleanup(clear);
  const ratio = () => duration() > 0 ? clamp(position() / duration(), 0, 1) : 0;
  const play = async (seekTo?: number) => {
    const current = target();
    if (!current || loading()) return;
    if (!claim?.current()) claim = mediaPlayback.take(clear, () => { if (loading()) clear(); else player?.pause(); });
    const mine = claim;
    if (seekTo !== undefined) { pendingSeek = seekTo; setPosition(seekTo); }
    setError("");
    try {
      if (!objectUrl) {
        setLoading(true);
        const bytes = await attachmentMedia(current.computer, current.ref, video);
        if (!mine.current() || target() !== current) return;
        objectUrl = URL.createObjectURL(recordingBlob(bytes, video));
        setUrl(objectUrl);
      }
      await Promise.resolve();
      if (mine.current() && target() === current && player) await player.play();
    } catch (failure) { if (mine.current()) { setError(`Could not play recording: ${String(failure)}`); setPlaying(false); } }
    finally { if (mine.current()) setLoading(false); }
  };
  const pause = () => { player?.pause(); setPlaying(false); };
  const loaded = (event: Event) => {
    if (!claim?.current() || !objectUrl) return;
    const media = event.currentTarget as HTMLMediaElement;
    if (Number.isFinite(media.duration) && media.duration > 0) setDuration(Math.round(media.duration * 1000));
    if (pendingSeek !== undefined) { media.currentTime = Math.min(pendingSeek, duration()) / 1000; pendingSeek = undefined; }
  };
  const moved = (event: Event) => { if (claim?.current() && objectUrl) setPosition(Math.round((event.currentTarget as HTMLMediaElement).currentTime * 1000)); };
  const failed = (event: Event) => {
    if (!claim?.current() || !objectUrl) return;
    setPlaying(false); setLoading(false);
    setError(`Could not play recording: ${(event.currentTarget as HTMLMediaElement).error?.message || "Recording is unavailable."}`);
  };
  const seek = (e: MouseEvent) => {
    if (!target() || !duration()) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const next = Math.round(clamp((e.clientX - r.left) / r.width, 0, 1) * duration());
    if (objectUrl && player && claim?.current()) { player.currentTime = next / 1000; setPosition(next); }
    else void play(next);
  };
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
          <div class="vp" style={{ "--p": String(ratio()) }}>
            <button class="vp-btn" aria-label={`Play or pause voice message (${mediaTime(duration())})`} aria-pressed={isPlaying()}
              disabled={!target() || loading()} title={!target() ? "Recording is unavailable." : undefined} aria-busy={loading()} onClick={() => { if (isPlaying()) pause(); else void play(); }}>
              <svg width="44" height="44" viewBox="0 0 44 44" aria-hidden="true">
                <Show when={isPlaying()} fallback={<path d="M17 12L31 22L17 32Z" />}>
                  <rect x="15" y="13" width="5" height="18" rx="1" /><rect x="24" y="13" width="5" height="18" rx="1" />
                </Show>
              </svg>
            </button>
            <div class="vp-side">
              <div class="vp-wave" role="slider" tabindex="-1" aria-label="Seek" aria-valuemin={0} aria-valuemax={duration()} aria-valuenow={position()} onClick={seek}>
                <Waveform values={it.waveform} />
              </div>
              <span class="vp-caption tabular">{mediaTime(position())} / {mediaTime(duration())}</span>
            </div>
          </div>
          <Show when={it.transcript}><TranscriptButton open={transcript()} onToggle={toggleTranscript} /></Show>
        </div>
      }>
        {/* The round video plays in place: click plays or pauses; a ring shows progress. */}
        <div class="vm-row video">
          <button class="vm-poster" classList={{ playing: isPlaying(), started: position() > 0 && position() < duration() }}
            style={{ "--p": String(ratio()) }} aria-pressed={isPlaying()}
            aria-label={`${isPlaying() ? "Pause" : "Play"} video message (${mediaTime(duration())})`} disabled={!target() || loading()} title={!target() ? "Recording is unavailable." : undefined} aria-busy={loading()} onClick={() => { if (isPlaying()) pause(); else void play(); }}>
            <span class="vm-frames" classList={{ real: !!url() }} aria-hidden="true">
              <Show when={url()}><video ref={(el) => { player = el; }} src={url()} playsinline preload="metadata"
                onLoadedMetadata={loaded} onTimeUpdate={moved} onPlaying={() => setPlaying(true)} onPause={() => setPlaying(false)} onEnded={() => setPlaying(false)} onError={failed} /></Show>
            </span>
            <svg class="vm-ring" viewBox="0 0 200 200" aria-hidden="true">
              <circle class="vm-ring-track" cx="100" cy="100" r="97" pathLength="100" />
              <circle class="vm-ring-done" cx="100" cy="100" r="97" pathLength="100" style={{ "stroke-dashoffset": String(100 - 100 * ratio()) }} />
            </svg>
            <span class="vm-play" aria-hidden="true">
              <svg width="48" height="48" viewBox="0 0 48 48">
                <Show when={isPlaying()} fallback={<path d="M18 12L36 24L18 36Z" />}><rect x="15" y="13" width="6" height="22" rx="1.5" /><rect x="27" y="13" width="6" height="22" rx="1.5" /></Show>
              </svg>
            </span>
            <span class="vm-time tabular">{position() > 0 ? `${mediaTime(position())} / ` : ""}{mediaTime(duration())}</span>
          </button>
          <Show when={it.transcript}><TranscriptButton open={transcript()} onToggle={toggleTranscript} /></Show>
        </div>
      </Show>
      <Show when={transcript() && it.transcript}><p class="vm-text">{it.transcript}</p></Show>
      <Show when={!video}><audio ref={(el) => { player = el; }} src={url() || undefined} preload="none"
        onLoadedMetadata={loaded} onTimeUpdate={moved} onPlaying={() => setPlaying(true)} onPause={() => setPlaying(false)} onEnded={() => setPlaying(false)} onError={failed} /></Show>
      <Show when={visibleError()}><p class="vm-error">{visibleError()}</p></Show>
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
  const contents = () => <>
    <Show when={props.file.image}>
      <span class="att-thumb"><Show when={thumbnail()}>{(url) => <img src={url()} alt="" onError={() => { const now=target(); if(now)previews.failed(now.computer,now.ref); }} />}</Show></span>
    </Show>
    <span class="att-caption">{props.file.caption}</span>
    <span class="dim">{props.file.meta}</span>
    <span class="att-open" style={{ visibility: target() ? "visible" : "hidden" }}>Open →</span>
  </>;
  return (
    <Show when={target()} fallback={<span class="att-card" title={`Attachment: ${props.file.name}`}>{contents()}</span>}>
      {(now) => <button class="att-card" title={props.file.caption} onClick={() => void openAttachment(now().computer,now().ref)}>{contents()}</button>}
    </Show>
  );
}

// ---------------------------------------------------------------------------
// Local message rows: Retry, Check Terminal (or native UI) and Delete
// (ActivityView.cpp:1059-1068, SessionsWindow.cpp messageAction 4470-4527).

/** The row whose Delete waits for its confirmation, and its Delete button, which the focus returns to on Cancel. */
const [deleting, setDeleting] = createSignal<{ id: string; from: HTMLElement }>();

function LocalActions(props: { local: LocalItem }) {
  // Retry stays focusable while off and says why (Rust refuses it while the composer is busy).
  const reason = (action: LocalItem["actions"][number]) => (action === "retry" ? retryReason(composer.view(), composer.operationLocked()) : "");
  const run = (action: LocalItem["actions"][number], from: HTMLElement) => {
    const id = props.local.id;
    if (reason(action)) return;
    if (action === "retry") void composer.localRetry(id);
    else if (action === "delete") setDeleting({ id, from });
    else void composer.localInspect(id).then((routed) => routeInspect(routed, {
      showTerminal: () => { terminal.opened(); setDetailTab("terminal"); },
      notify: (tone, text) => { notify(tone, text); },
    }));
  };
  const label = (action: LocalItem["actions"][number]) => (action === "retry" ? "Retry" : action === "delete" ? "Delete" : props.local.inspectLabel);
  return (
    <div class="act-local-actions">
      <For each={props.local.actions}>{(action) => (
        <button type="button" class="btn small" aria-disabled={reason(action) ? "true" : undefined} title={reason(action) || undefined}
          onClick={(e) => run(action, e.currentTarget)}>{label(action)}</button>
      )}</For>
    </div>
  );
}

/** "Delete message?": Cancel has the focus and is the default; Escape is Cancel. */
function LocalDeleteDialog() {
  let el: HTMLDialogElement | undefined;
  createEffect(() => {
    if (!el) return;
    if (!deleting()) { if (el.open) el.close(); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => el?.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  /** Cancel: the focus returns to the row's Delete. */
  const cancel = () => {
    const from = deleting()?.from;
    setDeleting(undefined);
    if (from?.isConnected) from.focus();
  };
  /** Delete: the row goes, and the focus moves to the transcript. */
  const confirm = () => {
    const id = deleting()?.id;
    setDeleting(undefined);
    if (id) void composer.localDelete(id);
    focusTranscript();
  };
  return (
    <dialog class="dlg" ref={el} role="alertdialog" aria-labelledby="local-delete-title" aria-describedby="local-delete-body" style={{ width: "440px" }}
      onCancel={(e) => { e.preventDefault(); cancel(); }} onClose={() => { if (!el?.open && deleting()) cancel(); }}>
      <div class="dlg-head"><Icon name="trash" size={18} class="danger" /><h2 id="local-delete-title">{DELETE_TITLE}</h2></div>
      <div class="dlg-body"><div class="confirm-body" id="local-delete-body">{DELETE_TEXT}</div></div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn primary" data-initial onClick={cancel}>Cancel</button>
        <button type="button" class="btn solid-danger" onClick={confirm}>Delete</button>
      </div>
    </dialog>
  );
}

function ItemBody(props: { item: ActivityItem; scope?: AttachmentScope }) {
  const it = props.item;
  return (
    <div class="act-body">
      <Show when={it.kind === "user"} fallback={<Body text={it.body ?? ""} />}>
        {/* A Codex question reply reads as Question and Your answer blocks (ActivityView.cpp:347-363). */}
        <Show when={it.replies?.length} fallback={<Body text={it.body ?? ""} />}>
          <For each={it.replies}>{(r) => (
            <div class="act-reply">
              <div class="act-reply-q">Question</div>
              <p class="wrap-pre">{r.question}</p>
              <div class="act-reply-a"><b>Your answer</b><p class="wrap-pre">{r.answer}</p></div>
            </div>
          )}</For>
        </Show>
      </Show>
      <Show when={it.local?.error}><p class="act-error">{it.local!.error}</p></Show>
      <Show when={it.code}><pre class="act-code mono">{it.code}</pre></Show>
      <Show when={it.attachments}>
        <div class="act-attachments">
          <For each={it.attachments}>{(a) => (
            // Reserve the same card geometry before its delivery receipt arrives.
            // A missing request remains read-only; it never authorizes a file read.
            <AttachmentCard file={a} scope={props.scope} />
          )}</For>
        </div>
      </Show>
      <Show when={it.local?.actions.length}><LocalActions local={it.local!} /></Show>
    </div>
  );
}

function EchoBody(props: { item: ActivityItem; scope?: AttachmentScope }) {
  const request = createMemo(() => {
    const files = props.item.attachments;
    if (!props.scope || !files?.length) return undefined;
    return { scope: props.scope, files };
  }, undefined, { equals: (a, b) => JSON.stringify(a) === JSON.stringify(b) });
  const [answer, setAnswer] = createSignal<{ current?: { scope: AttachmentScope; files: ItemAttachment[] }; done: boolean; ok: boolean }>({ done: false, ok: false });
  createEffect(on(request, (current) => {
    if (!current) { setAnswer({ current, done: true, ok: false }); return; }
    let live = true;
    setAnswer({ current, done: false, ok: false });
    void readCards(current.files, current.scope, attachmentFetch).then((files) => {
      if (live) setAnswer({ current, done: true, ok: !!files });
    });
    onCleanup(() => { live = false; });
  }));
  const state = () => {
    const current = request();
    if (!current) return "raw";
    const now = answer();
    if (now.current !== current || !now.done) return "loading";
    return now.ok ? "cards" : "raw";
  };
  return <>
    <Show when={state() === "cards"}><ItemBody {...props} /></Show>
    <Show when={state() === "raw"}><ItemBody {...props} item={{ ...props.item, body: props.item.echoBody, attachments: undefined }} /></Show>
    <Show when={state() === "loading"}><ItemBody {...props} item={{ ...props.item, attachments: undefined }} /></Show>
  </>;
}

function Item(props: { item: TranscriptItem; scope?: AttachmentScope }) {
  const [open, toggleOpen] = layoutDisclosure();
  const it = props.item;
  if (it.kind === "media") return <MediaCard item={it} scope={props.scope} />;
  if (it.kind === "context") return <div class="act-label eyebrow" id={`item-${it.id}`}>{it.body}</div>;
  if (it.kind === "thinking") return <div class="act-thinking" id={`item-${it.id}`}><span class="eyebrow">Thinking</span><p>{it.body}</p></div>;
  if (it.kind === "report")
    return (
      <div class="act-notice" id={`item-${it.id}`} classList={{ open: open() }}>
        <div class="row"><Icon name="bot" size={13} /><b>{it.title}</b><Show when={it.sender}><span class="muted">from {it.sender}</span></Show><span class="grow" /><span class="act-time">{it.at}</span></div>
        <button class="link" onClick={toggleOpen}>{open() ? "▾ Hide report" : "▸ Show report"}</button>
        <Show when={open()}><p class="act-notice-body">{it.body}</p></Show>
      </div>
    );
  return (
    <Show when={it.kind !== "tools" && it.kind !== "tool"} fallback={
      <div class="act-tools" id={`item-${it.id}`} classList={{ open: open(), warn: !!it.warn }}>
        <button class="act-tools-head" onClick={toggleOpen} aria-expanded={open()}>
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
          {/* A local row's stamp stands in for the time and the receipt (ActivityView.cpp:1053-1055). */}
          <Show when={it.local} fallback={<span class="act-time">{it.at}</span>}>
            {(l) => <span class="act-stamp" classList={{ failed: !!l().error || l().uncertain }}>{l().stamp}</span>}
          </Show>
          <Show when={it.kind === "user" && it.receipt}><span class="act-receipt"><Icon name="check" size={11} />{it.receipt}</span></Show>
          <span class="grow" />
          <Show when={it.kind === "agent"}><button class="icon-btn small act-copy" title="Copy as Markdown" onClick={() => copyText(it.body ?? "", "The reply is on the clipboard")}><Icon name="copy" size={12} /></button></Show>
        </header>
        <Show when={it.echoBody} fallback={<ItemBody item={it} scope={props.scope} />}>
          <EchoBody item={it} scope={props.scope} />
        </Show>
      </article>
    </Show>
  );
}

/** Within this distance of the top, scrolling reads the previous page. */
const OLDER_EDGE = 120;

// The jump to the latest activity (ActivityView.cpp 1607-1636): whether the
// view follows the newest item, how many events arrived since it stopped,
// and the jump back. The composer toolbar shows the jump while the view does
// not follow (Qt's useCompactJumpButton in its Latest slot).
const [following, setFollowing] = createSignal(true);
const [unseen, setUnseen] = createSignal(0);
/** The mounted transcript's scroller (one Activity view shows at a time). */
let liveScroller: HTMLDivElement | undefined;
let liveLatest: (() => void) | undefined;
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
  if (searchPreview()) { clearSearchPreview(); return; }
  setFollowing(true);
  setUnseen(0);
  liveLatest?.();
}

/** The transcript items of a source, with the open session's local rows. */
export const transcriptOf = (source: ActivitySource, local: readonly LocalMessage[] = []) => buildTranscript(source.events(), source.summary(), local);

/**
 * The local rows of the selected session's composer: none for a read-only
 * source, a subagent, or while the composer of another session still shows.
 */
export function localRows(source: ActivitySource, s: Session | undefined): readonly LocalMessage[] {
  const v = composer.view();
  if (!v || !s || source.readOnly || subagentOpen()) return [];
  const key = draftKey({ computer: s.computer, key: s.catalogKey, name: s.name, archive_id: s.state === "archived" ? s.archiveId ?? null : null });
  return v.key === key ? v.local : [];
}

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
  const preview = () => {
    const p = searchPreview(), row = s();
    return p && row && p.key === row.key && p.run === (row.runId ?? "") && p.conversation === (row.conversationId ?? "") && !subagentOpen() ? p : undefined;
  };
  /** The live follow unless the detail passes another source (an archived row's). */
  const source = (): ActivitySource => props.source ?? activity;
  let scroller!: HTMLDivElement;
  const [placed, setPlaced] = createSignal(false);
  const [layoutHeld, setLayoutHeld] = createSignal(false);
  // Items keep their identity while events append: the list reconciles by id,
  // so a growing tool group updates in place and older pages only add nodes.
  const [view, setView] = createStore<{ items: TranscriptItem[]; expired: boolean; loaded: boolean; pane: ReturnType<typeof paneStates>; tool: string }>({ items: [], expired: false, loaded: false, pane: paneStates(source(), s()), tool: "" });
  const local = createMemo(() => localRows(source(), s()), undefined, { equals: (a, b) => a === b || (a.length === 0 && b.length === 0) });
  const projection = createTranscriptProjection();
  let projectedTarget: string | undefined;
  const built = createMemo(() => {
    const target = source().target()?.id;
    if (target !== projectedTarget) { projection.reset(); projectedTarget = target; }
    return projection.build(source().events(), source().summary(), local());
  });
  createEffect(() => {
    const items = built(), loaded = source().loaded();
    const expired = source().olderDone() && source().events().length > 0 && source().summary()?.history_truncated === true;
    const pane = paneStates(source(), s());
    const summary = source().summary(), tool = String(summary?.current_tool || summary?.activity_summary || s()?.summary || "");
    resetTarget();
    holdSelection();
    motion.update(() => {
      settleReading(true);
      setView("items", reconcile(items, { key: "id" }));
      setView({ expired, loaded, pane, tool });
      reading.changed(); end.changed();
    });
  });

  // Keep a named row and its offset while the end pin is released.
  /** The newest event's seq the view has seen, for the count of new ones. */
  let newest: number | undefined;
  const selectionInside = () => {
    const selection = document.getSelection();
    if (!selection || selection.isCollapsed) return false;
    return [selection.anchorNode, selection.focusNode].some((node) => !!node && scroller.contains(node));
  };
  const end = createActivityEnd(
    () => ({ top: scroller.scrollTop, height: scroller.scrollHeight, viewport: scroller.clientHeight }),
    () => scroller.scrollTo({ top: scroller.scrollHeight, behavior: "instant" }),
    animationFrames,
    () => !motion.active() && !selectionInside(),
    () => { if (view.loaded || view.items.length) setPlaced(true); },
  );
  let latestAfterScroll = false;
  const motion = createActivityScroll(() => scroller.scrollTop, (flush) => {
    const atEnd = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight <= 1;
    const pinned = end.active();
    reading.remember();
    batch(() => { setLayoutHeld(false); flush(); });
    if ((pinned || atEnd || latestAfterScroll) && !preview()) { latestAfterScroll = false; setFollowing(true); setUnseen(0); reading.reset(); end.pin(); }
    else reading.changed();
  });
  const reading = createActivityAnchor({
    visible: () => activityReadingRows(scroller).visible(),
    offset: (key) => activityReadingRows(scroller).offset(key),
    adjust: (delta) => activityReadingRows(scroller).adjust(delta),
  }, () => !end.active() && !preview(), animationFrames, () => !motion.active() && !selectionInside());
  const holdSelection = () => {
    if (selectionInside()) { setLayoutHeld(true); motion.selection(true); }
  };
  const settleReading = (before = false) => { if (before) reading.beforeChange(); else reading.changed(); };
  const scrollIntent = () => {
    setLayoutHeld(true); motion.intent(); latestAfterScroll = false;
    end.release(); setFollowing(false); reading.remember();
  };
  const latest = () => {
    holdSelection();
    if (motion.active()) { latestAfterScroll = true; return; }
    reading.reset(); end.pin();
    scroller.scrollTo({ top: scroller.scrollHeight, behavior: "instant" });
  };
  onMount(() => {
    liveScroller = scroller;
    liveLatest = latest;
    const pointerEnded = () => motion.release();
    window.addEventListener("pointerup", pointerEnded); window.addEventListener("pointercancel", pointerEnded); window.addEventListener("blur", pointerEnded);
    onCleanup(() => { window.removeEventListener("pointerup", pointerEnded); window.removeEventListener("pointercancel", pointerEnded); window.removeEventListener("blur", pointerEnded); });
    const selectionChanged = () => {
      const inside = selectionInside();
      if (inside) setLayoutHeld(true);
      motion.selection(inside);
    };
    document.addEventListener("selectionchange", selectionChanged);
    // Content includes Markdown, images and tool disclosures; the viewport
    // also resizes when the composer or a question card changes height.
    const changed = () => { end.changed(); settleReading(); };
    const resize = new ResizeObserver(changed);
    resize.observe(scroller);
    const column = scroller.querySelector(".act-col");
    const mutations = new MutationObserver(changed);
    if (column) {
      resize.observe(column);
      mutations.observe(column, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ["class", "style", "src", "width", "height", "hidden", "open"] });
    }
    scroller.addEventListener("load", changed, true);
    scroller.addEventListener("contentvisibilityautostatechange", changed, true);
    document.fonts?.addEventListener("loadingdone", changed);
    onCleanup(() => { document.removeEventListener("selectionchange", selectionChanged); resize.disconnect(); mutations.disconnect(); scroller.removeEventListener("load", changed, true); scroller.removeEventListener("contentvisibilityautostatechange", changed, true); document.fonts?.removeEventListener("loadingdone", changed); });
  });
  onCleanup(() => {
    end.dispose(); reading.dispose(); motion.dispose();
    if (liveScroller === scroller) liveScroller = undefined;
    if (liveLatest === latest) liveLatest = undefined;
  });
  const onScroll = () => {
    if (motion.active()) {
      motion.moved(); reading.remember();
      if (!preview() && scroller.scrollTop < OLDER_EDGE && readsOlder(source())) source().loadOlder();
      return;
    }
    if (end.scrolled()) return;
    reading.changed();
  };
  let sendIds: Set<string> | undefined;
  let layoutTarget: string | undefined | null = null;
  const resetTarget = () => {
    const target = source().target()?.id;
    if (layoutTarget === target) return;
    layoutTarget = target;
    // An explicit session switch ends its old selection. Do this before the
    // first new projection, so a held range cannot defer a cached first page.
    if (selectionInside()) document.getSelection()?.removeAllRanges();
    sendIds = undefined; setPlaced(false);
    motion.reset(); setLayoutHeld(false); latestAfterScroll = false; setFollowing(true); setUnseen(0); reading.reset(); newest = undefined;
    // Selection changes must place even when the next page has the same row count.
    end.pin();
  };
  createEffect(on(() => source().target()?.id, resetTarget));
  createEffect(on(local, (rows) => {
    const sends = newLocalSends(sendIds, rows.map((row) => row.id));
    sendIds = sends.baseline;
    if (sends.added.length) jumpToLatest();
  }));
  createEffect(on(preview, (match) => {
    if (match) {
      setPlaced(true);
      motion.reset(); setLayoutHeld(false); end.release(); reading.reset(); setFollowing(false);
      requestAnimationFrame(() => { if (!motion.active() && !selectionInside()) scroller.querySelector(".search-preview mark")?.scrollIntoView({ block: "center" }); });
    } else { setFollowing(true); end.pin(); }
  }));
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


  const scope = createMemo(() => source().attachmentScope?.(), undefined, { equals: (a, b) => JSON.stringify(a) === JSON.stringify(b) });
  const liveTool = () => view.tool;
  const states = () => view.pane;
  const fileSession = () => {
    const id = source().target()?.id;
    return orderedRows().find((row) => row.key === id);
  };
  return (
    <ActivitySession.Provider value={fileSession}>
    <ActivityLayoutHeld.Provider value={layoutHeld}>
    <div class="activity" style={{ position: "relative" }}>
      <Show when={!placed()}>
        <div class="act-col" style={{ position: "absolute", inset: "0", "min-height": "0", "pointer-events": "none" }} role="status">
          <div class="act-label eyebrow">Recorded activity — messages and tool excerpts</div>
          <div class="act-older hint">Reading activity…</div>
        </div>
      </Show>
      <LocalDeleteDialog />
      <SessionFileDialog />
      <div class="act-scroll" ref={scroller} tabindex="-1" onContextMenu={activityMenu} onScroll={onScroll} style={{ "overflow-anchor": "none" }}
        onWheel={scrollIntent}
        onKeyDown={(e) => { if (!e.defaultPrevented && ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "].includes(e.key)
          && !(e.target as HTMLElement).closest("input,textarea,select,[contenteditable=true]") && !(e.key === " " && (e.target as HTMLElement).closest("button,a"))) { scrollIntent(); } }}
        onPointerDown={(e) => { if (e.button === 0 && !(e.target as HTMLElement).closest("button,a,input,textarea,.col-edge")) {
          // The scrollbar releases follow. Selecting text merely pauses it;
          // the persistent range keeps ownership after pointerup.
          if (e.target === scroller) scrollIntent();
          else setLayoutHeld(true);
          motion.hold();
        } }}
        onTouchStart={() => { scrollIntent(); motion.hold(); }} onTouchMove={scrollIntent} onTouchEnd={() => motion.release()} onTouchCancel={() => motion.release()}>
        <div class="act-col" style={{ visibility: placed() ? "visible" : "hidden" }}>
          <Show when={!fullWidth()}><ColumnEdge /><ColumnEdge right /></Show>
          <Show when={subagentOpen()}>
            <div class="subagent-bar">
              <button class="btn small" onClick={() => setSubagentOpen(undefined)}><Icon name="arrow-right" size={12} style={{ rotate: "180deg" }} />Main activity</button>
              <b>{s()?.subagents?.find((k) => k.id === subagentOpen())?.name}</b>
              <span class="hint">Showing recent activity. Read only: this agent is managed by the parent conversation.</span>
            </div>
          </Show>
          <Show when={preview()}>{(p) => <section class="search-preview" aria-label="Search result">
            <div class="act-label eyebrow">SEARCH RESULT — Saved message</div>
            <Show when={p().hit.content_truncated}><div class="hint">Excerpt around the match. This message is too long to display in full.</div></Show>
            <div class="search-preview-body"><SearchHighlight text={String(p().hit.event.detail ?? "")} query={p().query} /></div>
          </section>}</Show>
          <Show when={!preview()}>
          <div class="act-label eyebrow" style={{ position: "relative" }}>
            <span style={{ visibility: source().loadingOlder() ? "hidden" : "visible" }}>Recorded activity — messages and tool excerpts</span>
            <span class="act-older hint" style={{ position: "absolute", inset: "0", padding: "0", display: "flex", "align-items": "center", "justify-content": "center", "text-transform": "none", "letter-spacing": "normal", "font-weight": "normal", visibility: source().loadingOlder() ? "visible" : "hidden" }} aria-hidden={!source().loadingOlder()}>Reading earlier activity…</span>
          </div>
          <Show when={view.expired}>
            <div class="act-older hint">Older events have expired from the journal.</div>
          </Show>
          <Show when={!states().archived} fallback={<div class="notice"><Icon name="archive" size={14} /><div>This conversation is archived.</div></div>}>
            <Show when={view.loaded || view.items.length} fallback={<div class="act-older hint">Reading activity…</div>}>
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
          </Show>
        </div>
      </div>
    </div>
    </ActivityLayoutHeld.Provider>
    </ActivitySession.Provider>
  );
}
