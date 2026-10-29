import { batch, createSignal } from "solid-js";
import * as bridge from "../bridge";
import { notify } from "./actions";
import type { Timers } from "./terminal";
import type { Answer, ClearControl, ColdPrompt, ComposerView, SendReply, SessionRef, StartQuestion } from "../types";

// The open composer (Qt MessageComposer) over the Rust hub. The webview owns
// the text while the user types: edits go to Rust 250 ms after the last key,
// on blur and before any attach, paste, remove or send, each with a growing
// `rev` and the `text_epoch` the text is based on (Rust ignores an edit over
// older text). While the view has an error, an uncertain mark or a failed
// local row every key goes at once, since Rust may change the text itself
// then. Rust owns the attachments and the delivery state, and a growing
// `text_epoch` hands the text back (a load, an attach or remove, an Undo, a
// delivery's end, a refused change).
//
// The editor is read-only (Qt's `locked`, MessageComposer.cpp:1148-1149):
// while an attach or paste runs (one at a time, as Qt's synchronous attach);
// during a remove or Undo until a view with a higher epoch arrives; while a
// send or an answer is on its way and until Rust's view takes the lock over
// (sending, waiting) or hands the text back; while the cold cache or start
// question is open (it keeps the epoch, so closing it unlocks); while Rust
// says the draft is not loaded, is sending, attaching or waiting; and while
// the session status blocks input.

export const EDIT_MS = 250;
export const MAX_TEXT_BYTES = 64 * 1024;
/** Qt refuses a pasted image above this many pixels (MessageComposer.cpp:394; core `MAX_IMAGE_PIXELS`). */
export const MAX_PIXELS = 32_000_000;
export const TOO_LONG = "Message exceeds 64 KiB.";
const TOO_LARGE = "Image is too large. Use an image below 32 megapixels.";
const UNREADABLE = "Could not read the pasted image.";
/** How long the disabled Send's hint shows (Qt's QToolTip, MessageComposer.cpp:817). */
const HINT_MS = 5000;

export interface ComposerPort {
  open(session: SessionRef | undefined): Promise<ComposerView | null>;
  edit(handle: number, rev: number, epoch: number, text: string, position: number, anchor: number): Promise<void>;
  pick(handle: number, start: number, end: number): Promise<void>;
  pasteImage(handle: number, png: Uint8Array, start: number, end: number): Promise<void>;
  send(handle: number, rev: number, epoch: number, text: string, position: number, anchor: number): Promise<SendReply>;
  answer(token: number, answer: Answer): Promise<SendReply>;
  remove(handle: number, index: number): Promise<void>;
  undo(handle: number): Promise<void>;
  allowRetry(handle: number): Promise<void>;
  cancelSend(handle: number): Promise<void>;
  interrupted(handle: number): Promise<void>;
  clear(handle: number, run: string, conversation: string, dismiss: boolean): Promise<SendReply>;
  refresh(handle: number): Promise<void>;
}

/** Image decoding for pastes that are not PNG; tests pass their own. */
export interface Images {
  size(blob: Blob): Promise<{ width: number; height: number }>;
  png(blob: Blob): Promise<Uint8Array>;
}

/** Window notices (store/actions.ts `notify`); tests pass their own. */
export type Notices = (tone: "error" | "info", text: string, sticky: boolean) => void;

const browserImages: Images = {
  async size(blob) {
    const bitmap = await createImageBitmap(blob);
    const size = { width: bitmap.width, height: bitmap.height };
    bitmap.close();
    return size;
  },
  async png(blob) {
    const bitmap = await createImageBitmap(blob);
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    canvas.getContext("2d")!.drawImage(bitmap, 0, 0);
    bitmap.close();
    return new Uint8Array(await (await canvas.convertToBlob({ type: "image/png" })).arrayBuffer());
  },
};

const realTimers: Timers = { now: () => performance.now(), later: (fn, ms) => { const id = setTimeout(fn, ms); return () => clearTimeout(id); } };
const appNotices: Notices = (tone, text, sticky) => { notify(tone, text, sticky); };
const utf8 = new TextEncoder();

/** Qt's draft key: `machine\nsession[\narchive]` (src-tauri `draft_key`). */
export const draftKey = (s: SessionRef) => (s.archive_id ? `${s.computer}\n${s.name}\n${s.archive_id}` : `${s.computer}\n${s.name}`);

export type Dialog =
  | { kind: "cold"; token: number; prompt: ColdPrompt }
  | { kind: "start"; token: number; question: StartQuestion }
  /** The Clear session dialog keeps the control it showed; the confirm sends its identity back. */
  | { kind: "clear"; control: ClearControl };

/** The status line (Qt's updateControls 1200-1204): text and whether it is in the error colour. */
export interface StatusLine { text: string; error: boolean }

/**
 * The status line from Rust's view, the composer's own error and the length
 * of the text being typed. Rust orders storage error, waiting, the draft's
 * notice and the attachment reason above the length; the session status and
 * the block reason come after it.
 */
export function statusLine(v: ComposerView | undefined, own: string | undefined, tooLong: boolean): StatusLine {
  if (v?.waiting) return { text: v.status, error: v.status_error };
  if (own) return { text: own, error: true };
  if (!v) return tooLong ? { text: TOO_LONG, error: true } : { text: "", error: false };
  const below = v.status === "" || v.status === TOO_LONG || v.status === v.session_status || v.status === v.unavailable_reason;
  if (!below) return { text: v.status, error: v.status_error };
  if (tooLong) return { text: TOO_LONG, error: true };
  // Rust's length check may still see older text: the next item then.
  const text = v.status === TOO_LONG ? v.session_status || v.unavailable_reason : v.status;
  return { text, error: text !== "" && text === v.session_status && v.session_status_error };
}

export function createComposer(port: ComposerPort, timers: Timers = realTimers, images: Images = browserImages, notices: Notices = appNotices) {
  const [view, setView] = createSignal<ComposerView | undefined>();
  const [text, setText] = createSignal("");
  /** `start` is the anchor and `end` the cursor position (Qt's anchor and position); start may follow end. */
  const [selection, setSelection] = createSignal({ start: 0, end: 0 });
  /** An attach or paste runs. */
  const [pending, setPending] = createSignal(false);
  /** The composer's own error (a refused paste), Qt's `showError`: it stays until a hand-back, a send or Allow retry. */
  const [error, setError] = createSignal<string | undefined>();
  const [dialog, setDialog] = createSignal<Dialog | undefined>();
  /** The epoch a remove, Undo or refused send waits to see exceeded. */
  const [handBack, setHandBack] = createSignal<number | undefined>();
  /** A send or answer call is on its way. */
  const [calling, setCalling] = createSignal(false);
  /** A send started: locked until Rust's view shows it (sending, waiting) or hands the text back. */
  const [starting, setStarting] = createSignal(false);
  /** The disabled Send's hint (Qt shows the status as a tooltip at the editor). */
  const [hint, setHint] = createSignal<string | undefined>();
  let rev = 0;
  let epoch = 0;
  let dirty = false;
  let timer: (() => void) | undefined;
  let hintTimer: (() => void) | undefined;
  /** The draft key of the session last opened; a view of another composer is an older answer. */
  let expected: string | undefined;
  let opens = 0;
  let inFlight = 0;
  /** The newest envelope view while an open runs: it may be older than the open's answer. */
  let held: ComposerView | undefined;

  const handle = () => view()?.handle;
  const tooLong = () => utf8.encode(text()).length > MAX_TEXT_BYTES;
  const question = () => { const d = dialog(); return d?.kind === "cold" || d?.kind === "start"; };
  /** Qt's `operationLocked`: everything but the session status's lock. */
  const operationLocked = () => {
    const v = view();
    return pending() || handBack() !== undefined || calling() || starting() || question()
      || !v?.loaded || v.attaching || v.sending || !!v.waiting;
  };
  const locked = () => operationLocked() || !!view()?.session_blocks_input;
  const ordered = () => { const s = selection(); return { start: Math.min(s.start, s.end), end: Math.max(s.start, s.end) }; };
  /** Rust may change the text itself: an error, an uncertain mark or a failed local row. */
  const immediate = () => { const v = view(); return !!v && (v.status_error || v.uncertain || v.local.some((row) => row.status === "error")); };
  const hasContent = () => text().trim() !== "" || (view()?.attachments.length ?? 0) > 0;
  /** Qt's Send state (MessageComposer.cpp:1185), from Rust's view and the text being typed. */
  const sendEnabled = () => {
    const v = view();
    if (!v || !v.loaded || !v.available || v.uncertain || locked() || tooLong() || !hasContent()) return false;
    return !(v.attachments.length > 0 && !v.attachments_available);
  };

  /** The composer goes or changes: its open question answers Cancel, as its modal dialog would. */
  function dropDialog() {
    const d = dialog();
    setDialog(undefined);
    if (d && d.kind !== "clear") void port.answer(d.token, { kind: "cancel" }).catch(() => {});
  }

  function apply(next: ComposerView | undefined) {
    batch(() => {
      if (!next) {
        timer?.();
        timer = undefined;
        dirty = false;
        setView(undefined);
        setHandBack(undefined);
        setStarting(false);
        setError(undefined);
        dropDialog();
        return;
      }
      if (next.handle !== view()?.handle) {
        epoch = 0;
        rev = next.rev;
        dirty = false;
        timer?.();
        timer = undefined;
        setHandBack(undefined);
        setStarting(false);
        setError(undefined);
        dropDialog();
      }
      setView(next);
      if (next.text_epoch > epoch) {
        epoch = next.text_epoch;
        setHandBack(undefined);
        setStarting(false);
        rev = Math.max(rev, next.rev);
        dirty = false;
        setText(next.text);
        setSelection({ start: next.anchor, end: next.position });
        setError(undefined);
      }
      if (next.sending || next.waiting) setStarting(false);
    });
  }

  /** A view from the batch envelope: only the composer last opened, and after any open under way. */
  function receive(next: ComposerView) {
    if (inFlight > 0) { held = next; return; }
    if (expected === undefined || (next.key !== expected && next.handle !== handle())) return;
    apply(next);
  }

  async function flush() {
    timer?.();
    timer = undefined;
    const h = handle();
    if (!dirty || h === undefined) return;
    dirty = false;
    const s = selection();
    try { await port.edit(h, rev, epoch, text(), s.end, s.start); } catch { /* outside the app: nothing to keep */ }
  }

  function changed() {
    rev++;
    dirty = true;
    timer?.();
    timer = undefined;
    if (immediate()) void flush();
    else timer = timers.later(() => { void flush(); }, EDIT_MS);
  }

  function showHint(textToShow: string) {
    hintTimer?.();
    setHint(textToShow);
    hintTimer = timers.later(() => { hintTimer = undefined; setHint(undefined); }, HINT_MS);
  }

  /** Opens the Clear session dialog with the control shown now, while Rust allows the clear (Qt's clearContext checks). */
  function openClear() {
    const control = view()?.clear;
    if (control?.enabled && !dialog()) setDialog({ kind: "clear", control: { ...control } });
  }

  /** A reply of a send or an answer; `at` is the epoch it was made at, `refused` whether a refusal hands the text back. */
  function reply(r: SendReply, at: number, refused: boolean) {
    const v = view();
    switch (r.kind) {
      case "busy": case "blocked":
        // A refused Send hands its text back under a new epoch; an answer's refusal changes nothing.
        if (refused && epoch === at) setHandBack(at);
        break;
      case "sending":
        if (epoch === at && !v?.sending && !v?.waiting) setStarting(true);
        break;
      case "cold": setDialog({ kind: "cold", token: r.token, prompt: r.prompt }); break;
      case "start": setDialog({ kind: "start", token: r.token, question: r.question }); break;
      case "clear_context": openClear(); break;
      // Qt's showNotice(text, error): an error stays until dismissed, a plain notice goes (SessionsWindow.cpp:5210).
      case "notice": notices(r.error ? "error" : "info", r.text, r.error); break;
    }
  }

  /** Runs one attach or paste; a second while one runs does nothing. */
  async function exclusive(work: (h: number) => Promise<void>) {
    const h = handle();
    if (h === undefined || locked()) return;
    setPending(true);
    try { await flush(); await work(h); } catch { /* outside the app */ } finally { setPending(false); }
  }

  /** Remove and Undo change the text in Rust: locked until it hands the text back. */
  async function handBackAfter(call: (h: number) => Promise<void>) {
    const h = handle();
    if (h === undefined || locked()) return;
    await flush();
    const at = epoch;
    setHandBack(at);
    try { await call(h); } catch { if (epoch === at) setHandBack(undefined); }
  }

  return {
    view, text, selection, dialog, locked, operationLocked, sendEnabled, hint,
    /** The status line: Qt's order, with the composer's own error and the typed length. */
    status: (): StatusLine => statusLine(view(), error(), tooLong()),
    apply, receive,
    async open(session: SessionRef | undefined) {
      await flush();
      expected = session ? draftKey(session) : undefined;
      const mine = ++opens;
      inFlight++;
      let next: ComposerView | null = null;
      try { next = await port.open(session); } catch { next = null; } finally { inFlight--; }
      if (mine !== opens) return;
      apply(next ?? undefined);
      const later = held;
      held = undefined;
      if (later && inFlight === 0) receive(later);
    },
    input(next: string, start: number, end: number) {
      if (locked() || !view()) return;
      batch(() => { setText(next); setSelection({ start, end }); });
      changed();
    },
    /** The cursor moved: Qt saves the draft with its cursor (rememberCursor). */
    select(start: number, end: number) {
      const s = selection();
      if (s.start === start && s.end === end) return;
      setSelection({ start, end });
      if (!locked() && view()) changed();
    },
    flush,
    attachFiles: () => exclusive((h) => { const s = ordered(); return port.pick(h, s.start, s.end); }),
    async pasteImage(blob: Blob) {
      await exclusive(async (h) => {
        let png: Uint8Array;
        try {
          if (blob.type === "image/png") png = new Uint8Array(await blob.arrayBuffer());
          else {
            const { width, height } = await images.size(blob);
            if (width * height > MAX_PIXELS) { setError(TOO_LARGE); return; }
            png = await images.png(blob);
          }
        } catch { setError(UNREADABLE); return; }
        const s = ordered();
        await port.pasteImage(h, png, s.start, s.end);
      });
    },
    remove: (index: number) => handBackAfter((h) => port.remove(h, index)),
    undo: () => handBackAfter((h) => port.undo(h)),
    async send() {
      const h = handle();
      if (h === undefined || locked()) return;
      if (!sendEnabled()) {
        // Qt shows the status at the editor when there is something to send (MessageComposer.cpp:814-818).
        const status = statusLine(view(), error(), tooLong()).text;
        if (hasContent() && status) showHint(status);
        return;
      }
      hintTimer?.();
      setHint(undefined);
      await flush();
      const at = epoch;
      const s = selection();
      setCalling(true);
      setError(undefined);
      let r: SendReply;
      try { r = await port.send(h, rev, at, text(), s.end, s.start); } catch (e) {
        setCalling(false);
        notices("error", typeof e === "string" ? e : e instanceof Error ? e.message : String(e), true);
        return;
      }
      batch(() => { setCalling(false); reply(r, at, true); });
    },
    /** Answers the open question with its token. The question kept the epoch: closing it unlocks the editor. */
    async answer(answer: Answer) {
      const d = dialog();
      if (!d || d.kind === "clear") return;
      setDialog(undefined);
      await flush();
      const at = epoch;
      setCalling(true);
      let r: SendReply;
      try { r = await port.answer(d.token, answer); } catch { setCalling(false); return; }
      batch(() => { setCalling(false); reply(r, at, false); });
    },
    /** Cancel or Escape: a question answers Cancel; the Clear dialog just closes. */
    closeDialog: dropDialog,
    openClear,
    /** The Clear session confirmation: the dialog closes first, since the call answers only once the clear ended (up to 45 s). */
    async confirmClear() {
      const d = dialog();
      setDialog(undefined);
      const h = handle();
      if (h === undefined || d?.kind !== "clear") return;
      const at = epoch;
      let r: SendReply;
      try { r = await port.clear(h, d.control.run, d.control.conversation, d.control.dismiss); } catch (e) {
        notices("error", typeof e === "string" ? e : String(e), true);
        return;
      }
      reply(r, at, false);
    },
    async cancelSend() { const h = handle(); if (h !== undefined) await port.cancelSend(h).catch(() => {}); },
    async allowRetry() {
      const h = handle();
      if (h === undefined) return;
      setError(undefined);
      await port.allowRetry(h).catch(() => {});
    },
    /** Before a Stop interrupts: Rust cancels this composer's start or compaction (Qt 1187-1191). */
    async interrupted() { const h = handle(); if (h !== undefined) await port.interrupted(h).catch(() => {}); },
    /** The status line's Refresh (Qt's sessionRefreshRequested). */
    async refresh() { const h = handle(); if (h !== undefined && !operationLocked()) await port.refresh(h).catch(() => {}); },
  };
}

export type ComposerStore = ReturnType<typeof createComposer>;

export const composer = createComposer({
  open: bridge.composerOpen, edit: bridge.composerEdit, pick: bridge.composerPick, pasteImage: bridge.composerPasteImage,
  send: bridge.sendMessage, answer: bridge.sendAnswer, remove: bridge.composerRemove, undo: bridge.composerUndoRemove,
  allowRetry: bridge.composerAllowRetry, cancelSend: bridge.sendCancel, interrupted: bridge.sendInterrupted, clear: bridge.clearContext,
  refresh: bridge.composerRefresh,
});
