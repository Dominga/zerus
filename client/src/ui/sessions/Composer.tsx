import { replaceText } from "../lib/text-edit";
import { TextArea } from "../lib/TextField";
import { Show, Suspense, createEffect, createMemo, createSignal, lazy, on, onCleanup, onMount } from "solid-js";
import { createVoice, voicePort } from "../../store/voice";
import { VoiceToolbar, VoicePreview } from "./VoiceControls";
import { pageHidden } from "../../store/visibility";
import { flag } from "../../store/settings";
import { subagentOpen } from "../../store/ui";
import { Icon } from "../lib/icons";
import { composer } from "../../store/composer";
import { suggestions, suggestedInput } from "../../store/suggestions";
import { contentScale, selected } from "../../store/ui";
import { computerFresh, computerOffline } from "../../store/catalog";
import { copyReference, detailsFor, followedFor } from "../../store/actions";
import { onCommand, platform } from "../../store/shortcuts";
import { clipboardFiles } from "../../bridge";
import { pasteNative, pasteRoute, samePaste, type PasteBinding } from "./paste-files";
import { openInspectorCard } from "../../store/inspector";
import { ironRuleCounts, ironRulesChip } from "../../store/ironRuleCounts";
import { groupOf, INBOX } from "../../store/organization";
import { actionEntries } from "./ActionDialogs";
import { snapshotOf } from "./actions-ui";
import { markReadChip, runMarkRead } from "./mark-entries";
import { ProviderSelectionButton } from "./ProviderSelectionButton";
import { SettingsCheckBar } from "./SettingsCheckBar";
import { DeliveryHistoryHint } from "./DeliveryHistoryHint";
import { AttachmentsChip } from "./Attachments";
import { RecoveryChip } from "./RecoveryChip";
import { SessionUsageChip, UsageLimitChip } from "./UsageChips";
import { allowRetryShown, moveFocusOff } from "./composer-ui";
import { focusTranscript, following, jumpToLatest, shownSearchPreview, unseen } from "./Activity";
import { jumpButton } from "./search-hit";
import type { ChipTone } from "../../types";
import { tr } from "../../i18n";
import { CacheChip } from "./CacheChip";
import { trText } from "../../text";
import { machineCards } from "../../store/machines";
import type { ReferenceTarget } from "./FileReferenceDialog";
import { sameDraft, styleOfPlatform, withReference, type DraftBinding } from "./file-reference";

// The message composer (Qt MessageComposer, 300-366), after the mockup's
// structure: a top-edge grip, the 34 px toolbar, the editor (76-720 px) and the
// action row. The toolbar keeps Qt's slots (ComposerToolbar.h, Slot): the
// leading zone holds Attachments, the voice slot (Task 13), Mark as read,
// Latest, the compaction indicator and Cancel send; a plain spacer keeps the
// trailing zone right; it holds Recovery, Usage, Cache and Context. The action
// row holds Attach, Reference file, the status line, Refresh, Allow retry,
// the model button, Stop and Send (364). The store (store/composer.ts) owns
// the text and the locks; Rust owns the draft, the attachments and the send.

// Editor height at 100% content scale, 76-720 px, remembered per viewer.
const EDITOR = { initial: 76, min: 76, max: 720 };
const HEIGHT_KEY = "zerus-client-editor-height";
const savedHeight = (() => { try { return Number(localStorage.getItem(HEIGHT_KEY)) || 0; } catch { return 0; } })();
// Reference an existing file loads with its first use (zerus-5wmh.13.5).
const FileReferenceDialog = lazy(() => import("./FileReferenceDialog").then((m) => ({ default: m.FileReferenceDialog })));
const [editorHeight, setEditorHeightRaw] = createSignal(savedHeight >= EDITOR.min ? Math.min(EDITOR.max, savedHeight) : EDITOR.initial);
const setEditorHeight = (h: number) => {
  const v = Math.round(Math.max(EDITOR.min, Math.min(EDITOR.max, h)));
  setEditorHeightRaw(v);
  try { localStorage.setItem(HEIGHT_KEY, String(v)); } catch { /* storage may be unavailable */ }
};
/** Qt's double-click reset removes the stored height (MessageComposer.cpp:288-290). */
const resetEditorHeight = () => {
  setEditorHeightRaw(EDITOR.initial);
  try { localStorage.removeItem(HEIGHT_KEY); } catch { /* storage may be unavailable */ }
};

/** Below this width the toolbar shortens its chips (Qt's ComposerToolbar fit: short labels first). */
const COMPACT_WIDTH = 600;

const USUAL_STOP = () => tr("Interrupt the current turn, like Escape in Terminal. The session stays open; queued messages follow the agent's native behavior.");
const CANCEL_SEND = () => tr("Cancel the pending send and unlock the message for editing. Compaction or startup continues; use Stop to interrupt the agent.");
const CANCEL_LOCKED = () => tr("The desktop service is finishing this send; it can't be cancelled here.");
const ATTACH = () => tr("Attach files, paste an image, or drop files onto this session");

/**
 * ComposerResizeHandle (task 422): the height drag lives on the composer's top
 * border only - an invisible 6 px strip with a thin grip centred on the border
 * line, shown on hover, focus and drag. Drag up to grow the field, double-click
 * or Home to reset, Up and Down resize by 20 px (the handle is focusable). The
 * field never grows past the room above it: Activity keeps at least 120 px.
 */
export function ResizeHandle(props: { hidden?: boolean }) {
  const [dragging, setDragging] = createSignal(false);
  const limit = (el: HTMLElement) => {
    const pane = el.closest(".activity-wrap")?.getBoundingClientRect();
    const editor = el.closest(".composer")?.querySelector(".cmp-editor")?.getBoundingClientRect();
    if (!pane || !editor) return EDITOR.max;
    const activity = pane.height - (el.closest(".cmp-col")?.getBoundingClientRect().height ?? 0);
    return Math.max(EDITOR.min, Math.min(EDITOR.max, (editor.height + Math.max(0, activity - 120)) / contentScale()));
  };
  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const startY = e.clientY, start = editorHeight(), max = limit(el);
    setDragging(true);
    const move = (m: PointerEvent) => setEditorHeight(Math.min(max, start + (startY - m.clientY) / contentScale()));
    const up = () => { setDragging(false); el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); el.removeEventListener("pointercancel", up); };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  const key = (e: KeyboardEvent) => {
    const el = e.currentTarget as HTMLElement;
    if (e.key === "Home") resetEditorHeight();
    else if (e.key === "ArrowUp") setEditorHeight(Math.min(limit(el), editorHeight() + 20));
    else if (e.key === "ArrowDown") setEditorHeight(editorHeight() - 20);
    else return;
    e.preventDefault();
  };
  return (
    <div class="cmp-resize edge" classList={{ dragging: dragging() }} hidden={props.hidden} tabindex={0} role="separator" aria-orientation="horizontal"
      aria-label={tr("Message field height")} aria-valuemin={EDITOR.min} aria-valuemax={EDITOR.max} aria-valuenow={editorHeight()}
      onPointerDown={down} onDblClick={resetEditorHeight} onKeyDown={key}>
      <i class="cmp-grip" />
    </div>
  );
}

export function Composer() {
  const s = () => selected()!;
  const v = () => composer.view();
  const voice = createVoice(voicePort);
  onCleanup(() => { void voice.close(); });
  const child = () => !!v()?.subagent;
  // Reference an existing file (zerus-5wmh.13.5): bound to the draft it opened on, for the main composer only.
  const [reference, setReference] = createSignal<DraftBinding & { target: ReferenceTarget }>();
  const openReference = () => {
    const view = v(), session = s();
    if (!view || child() || composer.locked()) return;
    const card = machineCards[session.computer];
    setReference({ handle: view.handle, key: view.key, epoch: view.text_epoch, target: {
      local: session.computer === "", machine: session.machine, host: session.computer, style: styleOfPlatform(card?.platform) } });
  };
  /**
   * The sentence goes in as an ordinary edit, only into the draft the dialog
   * opened on: the same composer and text, and no send, attach or remove
   * under way. Otherwise nothing is inserted, as Qt's current() check.
   */
  const insertReference = (bound: DraftBinding, sentence: string): string | undefined => {
    if (!sameDraft(composer.view(), bound, composer.locked())) return undefined;
    const selection = composer.selection();
    const placed = withReference(composer.text(), selection.start, selection.end, sentence);
    if (!placed) return tr("Message exceeds 64 KiB.");
    composer.input(placed.text, placed.cursor, placed.cursor);
    requestAnimationFrame(() => { if (!editor.isConnected) return; editor.focus(); editor.setSelectionRange(placed.cursor, placed.cursor); });
    return undefined;
  };
  const online = () => !computerOffline(s().computer);
  const details = createMemo(() => detailsFor(s(), online(), followedFor(s().key)));
  createEffect(() => {
    const view = v(), row = s(), d = details();
    voice.follow(view ? { handle: view.handle, key: view.key, visible: !pageHidden() && !subagentOpen() && view.input_visible,
      revision: JSON.stringify([view.loaded, view.sending, view.waiting, view.attaching, view.available, view.block_reason, view.inspect_error, row.runId, row.conversationId, d?.run_id, d?.conversation_id,
        flag("speech/video_mode"), flag("speech/preview_before_send")]) } : undefined);
  });
  const act = createMemo(() => actionEntries(s(), details()));
  const mark = createMemo(() => markReadChip(s(), computerFresh(s().computer)));
  // Qt's compaction indicator (ActivityView.cpp 1899-1901) over the details it is given (3974-3975: an
  // offline host, a failed read or a session not running reads as unavailable).
  const compacting = createMemo(() => {
    const state = followedFor(s().key).value ?? {};
    return !child() && s().tracked && online() && !v()?.inspect_error && s().state === "running" && state.phase === "compacting" && state.runtime_state === "live"
      && state.process_state === "running" && state.state !== "archived";
  });
  createEffect(() => {
    const row = s(), state = followedFor(row.key).value ?? {};
    const raw = state.prompt_suggestion as { text?: unknown; at?: unknown } | undefined;
    const phase = typeof state.phase === "string" ? state.phase : row.phase;
    const active = !child() && phase === "idle" && online() && row.state === "running";
    suggestions.set(row.catalogKey, active && typeof raw?.text === "string" ? raw.text : "", typeof raw?.at === "number" ? raw.at : 0);
  });
  const suggestion = () => child() ? undefined : suggestions.get(s().catalogKey, !!v()?.sending || !!v()?.waiting);
  // Qt parity (I5b): the iron rules chip, from the count the Iron rules card
  // reports, else the project's rules read once for this session.
  const rulesChip = () => child() ? "" : ironRulesChip(ironRuleCounts.count(s().key));
  createEffect(on(() => s().key, (key) => {
    const group = groupOf(key), row = s();
    void ironRuleCounts.ensure(key, row.computer, group === INBOX ? "" : group ?? "");
  }));
  // Qt parity (I5c): Copy session reference, while online with a run.
  const referable = () => !child() && online() && !!(details()?.run_id || s().runId);
  const [compact, setCompact] = createSignal(false);
  let root!: HTMLDivElement;
  let editor!: HTMLTextAreaElement;

  /** The cursor as Qt keeps it: the anchor and the position. */
  const caret = () => editor.selectionDirection === "backward"
    ? { anchor: editor.selectionEnd, position: editor.selectionStart }
    : { anchor: editor.selectionStart, position: editor.selectionEnd };
  const remember = () => { const c = caret(); composer.select(c.anchor, c.position); };
  // A text Rust hands back (a higher epoch, another composer) brings its cursor; other views leave the field alone.
  const handedBack = createMemo(() => `${v()?.handle ?? ""}:${v()?.text_epoch ?? ""}`);
  createEffect(on(handedBack, () => {
    const sel = composer.selection();
    const start = Math.min(sel.start, sel.end), end = Math.max(sel.start, sel.end);
    try { editor.setSelectionRange(start, end, sel.start <= sel.end ? "forward" : "backward"); } catch { /* not rendered */ }
  }));

  const stopTurn = () => {
    if (child()) return false;
    const stop = act().stop;
    if (!stop.shown || stop.disabled) return false;
    // A refused interrupt keeps its turn's prompt with the turn (Qt 1204), bound to this Stop.
    void composer.interrupted().then((stopped) => stop.interrupt().then((out) => { if (out && out.outcome !== "done") void composer.interruptFailed(stopped); }));
    return true;
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.isComposing || e.keyCode === 229) return;
    if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey) {
      const next = suggestedInput(composer.text(), suggestion() ?? "", composer.locked());
      if (next && replaceText(editor, next)) {
        e.preventDefault();
        editor.setSelectionRange(next.length, next.length);
        composer.select(next.length, next.length);
      }
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void composer.send(); return; }
    // Like Escape in Terminal: stop a working turn, otherwise leave the field (ComposeEdit::keyPressEvent).
    if (e.key === "Escape") { e.preventDefault(); if (!stopTurn()) editor.blur(); }
  };
  /**
   * Qt's insertFromMimeData (MessageComposer.cpp 186-196): copied local files
   * are attached, otherwise an image is pasted, otherwise the text. A paste
   * without files stays the webview's own. Files without their paths ask the
   * native clipboard (bounded, off the UI thread); its answer applies only to
   * the draft the paste was made in.
   */
  const onPaste = (e: ClipboardEvent) => {
    if (!e.clipboardData) return;
    const route = pasteRoute(e.clipboardData, platform === "windows");
    if (route.kind === "text") return;
    e.preventDefault();
    if (route.kind === "paths") { void composer.attachPasted(route.paths); return; }
    const view = v();
    if (!view) return;
    const at = composer.selection();
    const bound: PasteBinding = { handle: view.handle, key: view.key, epoch: view.text_epoch, text: composer.text(), start: at.start, end: at.end };
    void pasteNative(route, {
      native: clipboardFiles,
      current: () => samePaste(composer.view(), bound, composer.locked(), composer.text(), composer.selection()),
      attach: (paths) => { void composer.attachPasted(paths); },
      image: (file) => { void composer.pasteImage(file); },
      text: (text) => { if (document.activeElement === editor) document.execCommand("insertText", false, text); },
    });
  };

  onMount(() => {
    const stopVoice = (event: KeyboardEvent) => {
      if (!voice.shown()?.recording || event.isComposing || document.querySelector("dialog[open]")) return;
      if (event.code !== "Space" && event.key !== "Escape") return;
      event.preventDefault(); event.stopImmediatePropagation();
      void voice.input({ kind: "stop", forget_send: false });
    };
    const blurVoice = () => { if (voice.shown()?.recording) void voice.input({ kind: "stop", forget_send: true }); };
    addEventListener("keydown", stopVoice, true); addEventListener("blur", blurVoice);
    onCleanup(() => { removeEventListener("keydown", stopVoice, true); removeEventListener("blur", blurVoice); });
    // Composer chords go through the window router (store/shortcuts.ts) while the focus is in the field.
    const stops = [onCommand("message.send", () => { void composer.send(); }, () => editor)];
    const observer = typeof ResizeObserver === "undefined" ? undefined : new ResizeObserver(() => setCompact(root.clientWidth < COMPACT_WIDTH * contentScale()));
    observer?.observe(root);
    onCleanup(() => { for (const stop of stops) stop(); observer?.disconnect(); void composer.flush(); });
  });
  createEffect(() => {
    if (!composer.pushEnabled()) return;
    // Remove the action when disabled, so Cut keeps its keys (Qt 841-853).
    const stop = onCommand("message.push", () => { void composer.push(); }, () => editor);
    onCleanup(stop);
  });

  const status = () => composer.status();
  const statusDetail = () => {
    const text = status().text, session = v()?.session_status ?? "";
    return session && session !== text ? `${text}\n${session}` : text;
  };
  // While a search hit shows, the button stays and goes back to the latest activity (ActivityView.cpp:1631-1636).
  const jump = () => jumpButton({ following: following(), unseen: unseen(), searchShown: !!shownSearchPreview() });
  const jumpCaption = () => jump().caption;
  const editorTip = () => v()?.waiting ?? (v()?.session_blocks_input ? v()!.session_status : undefined);
  // A required question card stands in for the editor, its grip and the action row; the toolbar stays (setInputVisible, 1126-1131).
  // They stay mounted and only hide, as Qt's setVisible: the editor's command, observer and flush need it.
  const input = () => v()?.input_visible !== false;
  return (
    <div class="composer" ref={root} data-shortcut-context="composer" classList={{ blocked: !!v()?.session_blocks_input }}>
      <ResizeHandle hidden={!input()} />
      <Show when={!child()}><SettingsCheckBar bar={v()?.settings_check} session={selected()} /></Show>
      <div class="cmp-toolbar">
        <AttachmentsChip compact={compact()} />
        <VoiceToolbar voice={voice} compact={compact()} />
        <Show when={!child() && mark().active}>
          <button type="button" class="tchip accent" classList={{ icon: compact() }} title={mark().detail} aria-label={tr("Mark as read")}
            onClick={(e) => {
              // The chip goes once read: its focus moves on first (composer-ui.ts moveFocusOff).
              if (document.activeElement === e.currentTarget) moveFocusOff(focusTranscript, editor);
              runMarkRead(snapshotOf(s()));
            }}>
            <Icon name="check-check" size={13} /><Show when={!compact()}>{tr("Mark as read")}</Show>
          </button>
        </Show>
        <Show when={jump().visible}>
          <button type="button" class="tchip icon" title={jumpCaption()} aria-label={jumpCaption()}
            onClick={(e) => { if (document.activeElement === e.currentTarget) moveFocusOff(focusTranscript, editor); jumpToLatest(); }}><Icon name="chevrons-down" size={14} /></button>
        </Show>
        <Show when={!child() && compacting()}>
          <span class="tchip cmp-compaction" title={tr("The agent is compressing conversation context.")} role="status">
            <span class="cmp-progress" aria-label={tr("Compacting context")} />{tr("Compacting context…")}
          </span>
        </Show>
        <Show when={!child() && v()?.cancel_send}>
          <button type="button" class="tchip link" disabled={v()?.cancel_locked} title={v()?.cancel_locked ? CANCEL_LOCKED() : CANCEL_SEND()} onClick={() => { void composer.cancelSend(); editor.focus(); }}>{tr("Cancel send")}</button>
        </Show>
        <span class="grow" />
        <Show when={!child()}>
          <RecoveryChip session={s()} compact={compact()} />
          <UsageLimitChip session={s()} compact={compact()} />
          <Show when={v()}><CacheChip compact={compact()} /></Show>
        </Show>
        <SessionUsageChip />
        <Show when={referable()}>
          <button type="button" class="tchip icon cmp-reference" title={tr("Copy session reference")} aria-label={tr("Copy session reference")}
            onClick={() => { void copyReference(snapshotOf(s()), details()); }}><Icon name="link" size={13} /></button>
        </Show>
      </div>
      <VoicePreview voice={voice} />
      <TextArea ref={editor} historyKey={handedBack()} class="cmp-editor" hidden={!input()} style={{ height: `${editorHeight() * contentScale()}px` }} aria-label={tr("Message to selected agent")}
        placeholder={suggestion() || tr("Message this agent…")} readOnly={composer.locked()} disabled={!v()} value={composer.text()} title={editorTip()}
        onInput={(e) => { const c = caret(); composer.input(e.currentTarget.value, c.anchor, c.position); }}
        onSelect={remember} onKeyUp={remember} onMouseUp={remember}
        onBlur={() => { void composer.flush(); }} onKeyDown={onKey} onPaste={onPaste} />
      <Show when={input() && composer.hint()}><div class="cmp-hint" role="status">{composer.hint()}</div></Show>
      <DeliveryHistoryHint view={v()} text={composer.text()} shown={input()} />
      <div class="cmp-actions" hidden={!input()}>
        <span class="cmp-files">
          <button type="button" class="icon-btn cmp-attach" disabled={!v() || composer.locked() || !v()!.attachments_available} aria-label={tr("Attach files to message")}
            title={v()?.attachments_available === false ? v()!.attachments_reason : ATTACH()}
            onClick={() => { void composer.attachFiles(); }}><Icon name="paperclip" size={17} /></button>
          <Show when={!child()}>
            <button type="button" class="icon-btn cmp-attach cmp-reference" disabled={!v() || composer.locked()} aria-label={tr("Reference an existing file")}
              title={tr("Reference an existing file")} onClick={openReference}><Icon name="file-text" size={16} /></button>
          </Show>
          <Show when={rulesChip()}>
            <button type="button" class="tchip cmp-rules" aria-label={rulesChip()} title={tr("Iron rules apply at session start and each team task; adapters without startup delivery also receive them with messages")}
              onClick={() => openInspectorCard({ id: "details-rules" }, { id: "sec-rules", folded: true })}><Icon name="shield-check" size={12} />{rulesChip()}</button>
          </Show>
        </span>
        <Show when={reference()} keyed>{(bound) => (
          <Suspense><FileReferenceDialog target={bound.target} insert={(sentence) => insertReference(bound, sentence)} close={() => setReference(undefined)} /></Suspense>
        )}</Show>
        <Show when={status().text}>
          <span class="cmp-status ell" classList={{ error: status().error, waiting: !status().error && !!v()?.waiting }} title={statusDetail()} aria-label={status().text}>{status().text}</span>
        </Show>
        <span class="grow" />
        <Show when={v()?.session_status && v()!.session_status_error}>
          <button type="button" class="icon-btn cmp-attach" aria-label={tr("Refresh this session")} title={`${tr("Refresh this session")}\n${v()!.session_status}`}
            disabled={composer.operationLocked()} onClick={() => { void composer.refresh(); }}><Icon name="refresh-cw" size={17} /></button>
        </Show>
        <Show when={allowRetryShown(v(), composer.operationLocked())}>
          <button type="button" class="btn ghost" title={tr("Check the terminal before sending again to avoid a duplicate message")}
            onClick={() => { void composer.allowRetry(); editor.focus(); }}><Icon name="refresh-cw" size={13} />{tr("Allow retry")}</button>
        </Show>
        <Show when={!child()}><ProviderSelectionButton session={s()} compact={compact()} /></Show>
        <Show when={!child() && act().stop.shown}>
          <button type="button" class="btn" aria-label={tr("Stop current turn")} title={act().stop.hint ?? USUAL_STOP()} disabled={act().stop.disabled}
            onClick={() => { stopTurn(); }}><Icon name="square" size={12} />{tr("Stop")}</button>
        </Show>
        <button type="button" class="btn primary" aria-label={tr("Send")} disabled={!composer.sendEnabled()} title={v()?.waiting ?? undefined}
          onClick={() => { void composer.send(); }}>{v() ? trText(v()!.send_label) : tr("Send")}<Icon name="arrow-up" size={13} /></button>
      </div>
    </div>
  );
}
