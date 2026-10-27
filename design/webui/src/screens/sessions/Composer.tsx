import { Show, createSignal } from "solid-js";
import { Icon } from "../../lib/icons";
import { selected, toast, setDialog } from "../../state";
import { accounts } from "../../mock/data";

// Composer structure from MessageComposer: resize handle, a 34px toolbar
// (leading: attachments, voice, latest, mark as read, compaction; trailing:
// usage limit, recovery, cache, context), the editor (76px default) and the
// action row (attach, reference file, status line, model, Stop, Send).

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
      <div class="cmp-resize" title="Drag to resize. Double-click to reset." />
      <div class="cmp-toolbar">
        <Show when={attached()}>
          <button class="tchip" onClick={() => setAttached(0)} title="Attachments: preview, open or remove"><Icon name="paperclip" size={13} />{attached()} attached</button>
        </Show>
        <button class="tchip" classList={{ rec: recording() }} onClick={() => setRecording(!recording())} title="Record a voice message (Space / Esc to stop)">
          <Icon name={recording() ? "square" : "mic"} size={13} />{recording() ? "Stop (0:07)" : "Voice"}
        </button>
        <Show when={recording()}><button class="tchip" title="Switch to video message"><Icon name="video" size={13} /></button></Show>
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
      <textarea class="cmp-editor" placeholder={offline() ? "Machine unavailable. Your draft stays here." : "Message this agent…"} value={text()} onInput={(e) => setText(e.currentTarget.value)}
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
