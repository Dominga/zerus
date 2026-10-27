import { For, Show, createEffect, createSignal, on } from "solid-js";
import { Icon } from "../../lib/icons";
import { setDialog } from "../../state";
import {
  addMessage, busy, canLeave, checkRecovery, describing, descriptionDraft, noteOnly, queued, recording, saveDescription, setDescribing,
  setDescriptionDraft, setNoteOnly, setRecording, taskDemo, workerLabel, type TaskFile, type TaskVM,
} from "./board";
import { below, openTaskMenu } from "./overlays";

// Private drafts keyed by project, task and input kind (comment / description).
const [drafts, setDrafts] = createSignal<Record<string, { text: string; files: TaskFile[] }>>({});

/**
 * The Task View input: the same composer skin as Activity. Its mode chip names
 * what Send does (queue context for Start, send feedback, request changes,
 * reopen, or save a note); Edit description switches it to the description
 * document with Save/Cancel. A saved comment awaiting a receipt offers
 * "Recover saved comment", which never sends it again.
 */
export function TaskComposer(props: { task: TaskVM }) {
  let editor!: HTMLTextAreaElement;
  const t = () => props.task;
  const desc = () => describing() && queued(t().stage) && t().writable;
  const key = () => `${desc() ? "description" : "comment"}\n${t().project}\n${t().id}`;
  const draft = () => drafts()[key()] ?? { text: "", files: [] };
  const setDraft = (d: { text: string; files: TaskFile[] }) => setDrafts({ ...drafts(), [key()]: d });
  const [error, setError] = createSignal(taskDemo === "attachments" ? "Task attachments could not be read." : "");

  // Edit description seeds the composer with the canonical document once.
  createEffect(on(desc, (on) => {
    if (!on) return;
    if (!drafts()[key()]) setDraft({ text: t().text, files: [...t().files] });
    queueMicrotask(() => editor?.focus());
  }));
  // An unsaved description blocks Start ("Save the description before starting this task").
  createEffect(() => {
    if (!desc()) { if (descriptionDraft() === t().id) setDescriptionDraft(undefined); return; }
    const d = draft();
    const changed = d.text !== t().text || d.files.length !== t().files.length;
    setDescriptionDraft(changed ? t().id : undefined);
  });

  const w = () => workerLabel(t());
  const primary = () => t().stage === "active" ? "Send to current executor" : t().stage === "review" ? "Request changes and send" : t().stage === "done" ? "Reopen and send" : "Add to task · send on Start";
  const modeLabel = () => noteOnly() ? "Save note only · no delivery" : queued(t().stage) ? "Add to task · sent on Start"
    : t().stage === "active" ? `Send to ${w()}` : t().stage === "review" ? `Request changes and send to ${w()}` : `Reopen and send to ${w()}`;
  const sendLabel = () => desc() ? "Save description" : noteOnly() || queued(t().stage) ? "Add to task" : "Send feedback";
  const placeholder = () => !t().writable ? "Update Zerus to comment on this task" : desc() ? "Describe the task…" : noteOnly() ? "Write a note for this task…" : queued(t().stage) ? "Add context for the executor…" : "Message this task’s executor…";

  const send = () => {
    if (!t().writable || busy()) return;
    const d = draft();
    if (desc()) {
      const rest = { ...drafts() };
      delete rest[key()];
      setDrafts(rest);
      saveDescription(t().id, d.text, d.files);
      return;
    }
    if (!d.text.trim() && !d.files.length) return;
    if (t().recovery) { setError("This task already has a message operation in progress."); return; }
    setError("");
    addMessage(t().id, d.text.trim(), d.files);
    setDraft({ text: "", files: [] });
  };
  const attach = () => {
    const d = draft();
    if (d.files.length >= 8) { setError("Attach up to 8 nonempty files: 10 MiB each, 20 MiB per task."); return; }
    const n = d.files.filter((f) => f.kind === "file").length + 1;
    const file: TaskFile = { ref: `[File #${n}]`, name: n === 1 ? "notes.txt" : `notes-${n}.txt`, size: "2 KB", kind: "file" };
    setDraft({ text: `${d.text}${d.text && !d.text.endsWith(" ") ? " " : ""}${file.ref} `, files: [...d.files, file] });
    editor.focus();
  };
  const modeMenu = (el: Element) => openTaskMenu(below(el), "Message mode", [
    { label: primary(), checked: !noteOnly(), action: () => setNoteOnly(false) },
    { label: "Save note only", checked: noteOnly(), action: () => setNoteOnly(true) },
  ]);
  const recoveryMenu = (e: MouseEvent) => {
    e.preventDefault();
    openTaskMenu({ x: e.clientX, y: e.clientY }, "Recovery", [
      { label: "Check delivery receipt", icon: "refresh-cw", action: () => checkRecovery(t().id) },
      { label: "Release local recovery record…", icon: "trash", action: () => setDialog("task-release-recovery") },
    ]);
  };

  return (
    <div class="tv-composer-wrap">
      <div class="composer tv-composer" classList={{ disabled: !t().writable, describing: desc() }}>
        <div class="cmp-resize" title="Drag to resize. Double-click to reset." />
        <div class="cmp-toolbar">
          <Show when={desc()} fallback={
            <button class="tchip tv-mode" disabled={!t().writable || !canLeave()} title={modeLabel()} aria-haspopup="menu" onClick={(e) => modeMenu(e.currentTarget)}>
              <Icon name={noteOnly() ? "file-text" : queued(t().stage) ? "clock" : "send"} size={12} /><span class="ell">{modeLabel()}</span><Icon name="chevron-down" size={12} />
            </button>
          }>
            <span class="tchip accent tv-mode" title="Save updates the canonical task document. Comments remain separate."><Icon name="pencil" size={12} />Editing description</span>
            <button class="tchip" onClick={() => { if (canLeave()) setDescribing(false); }}>Cancel</button>
          </Show>
          <Show when={draft().files.length}>
            <button class="tchip" title="Attachments: preview, open or remove" onClick={() => setDraft({ text: draft().text.replace(/\[(File|Image) #\d+\] ?/g, ""), files: [] })}>
              <Icon name="paperclip" size={12} />{draft().files.length} attached<Icon name="x" size={11} />
            </button>
          </Show>
          <button class="tchip" classList={{ rec: recording() }} disabled={!t().writable} onClick={() => setRecording(!recording())}
            title={recording() ? "Stop recording; the transcript goes into the draft for review" : "Record a voice note into this draft"}>
            <Icon name={recording() ? "square" : "mic"} size={12} />{recording() ? "Stop (0:07)" : "Voice"}
          </button>
          <Show when={t().recovery && !desc()}>
            <button class="tchip warn" title="Keep the previous comment in the task and check its delivery. It will not be sent again."
              disabled={busy()} onClick={() => checkRecovery(t().id)} onContextMenu={recoveryMenu}>
              <Icon name={busy() ? "loader-circle" : "rotate-ccw"} size={12} class={busy() ? "tk-spin" : undefined} />Recover saved comment
            </button>
          </Show>
          <span class="grow" />
          <button class="tchip icon" title="Jump to the latest discussion" onClick={() => document.querySelector(".tv-scroll")?.scrollTo({ top: 1e6, behavior: "smooth" })}><Icon name="chevrons-down" size={13} /></button>
        </div>
        <textarea ref={editor} class="cmp-editor" placeholder={placeholder()} value={draft().text} disabled={!t().writable}
          aria-label={desc() ? "Task description" : "Comment on this task"}
          onInput={(e) => setDraft({ ...draft(), text: e.currentTarget.value })}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); } }} />
        <div class="cmp-actions">
          <button class="icon-btn cmp-attach" title="Attach files (up to 8 files, 10 MiB each, 20 MiB per task)" aria-label="Attach files" disabled={!t().writable} onClick={attach}><Icon name="paperclip" size={17} /></button>
          <span class="cmp-status ell">{!t().writable ? "Update Zerus to comment on this task" : desc() ? "Save updates the canonical task document. Comments remain separate." : "Enter to send; Shift+Enter for a new line"}</span>
          <span class="grow" />
          <Show when={desc()} fallback={
            <button class="btn primary tv-send" disabled={!t().writable || busy() || (!draft().text.trim() && !draft().files.length)} title={sendLabel()} aria-label={sendLabel()} onClick={send}>
              <Show when={busy()} fallback={<Icon name={noteOnly() || queued(t().stage) ? "plus" : "send"} size={14} />}><Icon name="loader-circle" size={14} class="tk-spin" /></Show>
            </button>
          }>
            <button class="btn primary" disabled={busy()} onClick={send}>Save description</button>
          </Show>
        </div>
      </div>
      <Show when={error()}><div class="tv-note-error" role="alert">{error()}</div></Show>
      <Show when={desc() && draft().files.length}>
        <div class="tv-draft-files"><For each={draft().files}>{(f) => <span class="att-chip"><Icon name={f.kind === "image" ? "image" : "file-text"} size={13} />{f.ref} {f.name}</span>}</For></div>
      </Show>
    </div>
  );
}
