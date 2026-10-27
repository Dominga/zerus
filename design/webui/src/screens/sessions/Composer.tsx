import { Show, createSignal } from "solid-js";
import { Icon } from "../../lib/icons";
import { selected, toast, setDialog, contentScale } from "../../state";
import { accounts } from "../../mock/data";

// Composer structure from MessageComposer: resize handle, a 34px toolbar
// (leading: attachments, voice, voice mode, latest, mark as read, compaction;
// trailing: usage limit, recovery, cache, context), the editor (76px default)
// and the action row (attach, reference file, status line, model, Stop, Send).

// Editor height at 100% content scale, 76-720 px (workspace/messageEditorHeight).
const EDITOR = { initial: 76, min: 76, max: 720 };
const savedHeight = (() => { try { return Number(localStorage.getItem("zerus-mock-editor-height")) || 0; } catch { return 0; } })();
const [editorHeight, setEditorHeightRaw] = createSignal(savedHeight >= EDITOR.min ? Math.min(EDITOR.max, savedHeight) : EDITOR.initial);
const setEditorHeight = (h: number) => {
  const v = Math.round(Math.max(EDITOR.min, Math.min(EDITOR.max, h)));
  setEditorHeightRaw(v);
  try { localStorage.setItem("zerus-mock-editor-height", String(v)); } catch { /* ignore */ }
};
// Video mode is remembered like speech/video_mode.
const [videoMode, setVideoModeRaw] = createSignal((() => { try { return localStorage.getItem("zerus-mock-video-mode") === "1"; } catch { return false; } })());
const setVideoMode = (on: boolean) => { setVideoModeRaw(on); try { localStorage.setItem("zerus-mock-video-mode", on ? "1" : "0"); } catch { /* ignore */ } };

/** ComposerResizeHandle: a 10 px strip with a centred grip. Drag up to grow the
 *  field, double-click or Home to reset, Up/Down resize by 20 px. The field
 *  never grows past the room above it, so Activity keeps at least 120 px. */
function ResizeHandle() {
  const [dragging, setDragging] = createSignal(false);
  const limit = (el: HTMLElement) => {
    const pane = el.closest(".activity-wrap")?.getBoundingClientRect();
    const editor = el.parentElement?.querySelector(".cmp-editor")?.getBoundingClientRect();
    if (!pane || !editor) return EDITOR.max;
    // Room = current editor height plus whatever Activity can give up above 120 px.
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
    if (e.key === "Home") setEditorHeight(EDITOR.initial);
    else if (e.key === "ArrowUp") setEditorHeight(Math.min(limit(el), editorHeight() + 20));
    else if (e.key === "ArrowDown") setEditorHeight(editorHeight() - 20);
    else return;
    e.preventDefault();
  };
  return (
    <div class="cmp-resize" classList={{ dragging: dragging() }} tabindex="0" role="separator" aria-orientation="horizontal"
      aria-label="Message field height" aria-valuemin={EDITOR.min} aria-valuemax={EDITOR.max} aria-valuenow={editorHeight()}
      data-tip="Drag to resize the message field. Double-click to restore the default height."
      onPointerDown={down} onDblClick={() => setEditorHeight(EDITOR.initial)} onKeyDown={key}>
      <i class="cmp-grip" />
    </div>
  );
}

export function Composer() {
  const s = () => selected();
  const [text, setText] = createSignal("");
  const [recording, setRecording] = createSignal(false);
  const [attached, setAttached] = createSignal(1);
  const account = () => accounts.find((a) => a.id === s()?.account);
  const working = () => s()?.status === "working";
  const offline = () => s()?.status === "offline";
  const ctx = () => s()?.contextPct ?? 22;
  const send = () => {
    if (offline()) return;
    toast("Sent", "Submitted to the agent; delivery is confirmed in Activity");
    setText("");
  };
  return (
    <div class="composer" classList={{ disabled: offline() }}>
      <ResizeHandle />
      <div class="cmp-toolbar">
        <Show when={attached()}>
          <button class="tchip" onClick={() => setAttached(0)} title="Attachments: preview, open or remove"><Icon name="paperclip" size={13} />{attached()} attached</button>
        </Show>
        <button class="tchip" classList={{ rec: recording() }} onClick={() => setRecording(!recording())}
          title={recording() ? "Stop recording (Space / Esc)" : videoMode() ? "Click to start a video recording." : "Click to start a voice recording."}>
          <Icon name={recording() ? "square" : videoMode() ? "video" : "mic"} size={13} />{recording() ? "Stop (0:07)" : videoMode() ? "Video" : "Voice"}
        </button>
        {/* The mode switch is always there and shows the other mode; it locks while recording. */}
        <button class="tchip icon" disabled={recording()} onClick={() => setVideoMode(!videoMode())}
          data-tip={videoMode() ? "Switch to voice recording" : "Switch to video recording"} aria-label={videoMode() ? "Switch to voice recording" : "Switch to video recording"}>
          <Icon name={videoMode() ? "mic" : "video"} size={13} />
        </button>
        <Show when={s()?.unread}><button class="tchip accent" onClick={() => toast("Marked as read", "Reply marked as read")}><Icon name="check-check" size={13} />Mark as read</button></Show>
        <button class="tchip icon" title="Jump to latest activity (2 new)"><Icon name="chevrons-down" size={13} /></button>
        <span class="grow" />
        <Show when={s()?.status === "error"}>
          <button class="tchip danger" title="Automatic recovery"><Icon name="rotate-ccw" size={13} />Retry 14:20</button>
        </Show>
        <Show when={account() && account()!.fiveHour > 50}><button class="tchip warn" title="Usage limit for this account"><Icon name="gauge" size={13} />Usage {account()!.fiveHour}%</button></Show>
        <button class="tchip ok" title="Prompt cache stays warm for about 54 minutes"><Icon name="timer" size={13} />Cache ~54m</button>
        <button class="tchip" classList={{ warn: ctx() >= 70, danger: ctx() >= 90 }} title="Context used: open Details" onClick={() => toast("Details", "Opens the Details tab with session usage")}>
          <span class="ctx-ring" style={{ "--p": `${ctx()}%` }} />{ctx()}% (72k/200k)
        </button>
      </div>
      <textarea class="cmp-editor" style={{ height: `${editorHeight() * contentScale()}px` }} placeholder={offline() ? "Machine unavailable. Your draft stays here." : "Message this agent…"} value={text()} onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); } }} />
      <div class="cmp-actions">
        <button class="icon-btn cmp-attach" title="Attach files (up to 8 files, 10 MiB each)"><Icon name="paperclip" size={17} /></button>
        <button class="icon-btn cmp-attach" title="Reference an existing file without uploading it" onClick={() => setDialog("reference-file")}><Icon name="file-text" size={16} /></button>
        <span class="cmp-status ell">{offline() ? "Machine unavailable. Your draft stays here." : "Enter to send; Shift+Enter for a new line"}</span>
        <span class="grow" />
        <button class="btn ghost cmp-model" onClick={() => setDialog("model")} title="Model and reasoning effort">
          {s()?.model} <span class="dim">{s()?.effort}</span><Icon name="chevron-down" size={12} />
        </button>
        <Show when={working()}>
          <button class="btn" title="Interrupt the current turn, like Escape in Terminal" onClick={() => toast("Interrupt sent", "Waiting for the agent to stop…")}><Icon name="square" size={12} />Stop</button>
        </Show>
        <button class="btn primary" disabled={offline()} onClick={send}>Send<Icon name="arrow-up" size={13} /></button>
      </div>
    </div>
  );
}
