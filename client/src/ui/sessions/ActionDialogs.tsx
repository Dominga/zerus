import { For, Show, createEffect, createMemo, createSignal, untrack } from "solid-js";
import { Icon } from "../lib/icons";
import { catalog, computerOffline } from "../../store/catalog";
import { change, copyCommand, copyFolder, copyName, copyReference, dismiss, end, interrupting, notices, perform, reading, updating } from "../../store/actions";
import type { MenuEntry } from "../../store/ui";
import type { Details } from "../../types";
import type { Session } from "./types";
import { copyState, dialogReducer, menuState, renameParts, renameProblem, snapshotOf, terminateCopy, type ActionRow, type DialogEvent } from "./actions-ui";

// The guarded session actions: the entries both menus share, the Rename and
// Terminate dialogs, and the notices the action store raises.

const [open, dispatch] = createSignal<ReturnType<typeof dialogReducer>>(undefined, { equals: false });
const send = (event: DialogEvent) => dispatch(dialogReducer(open(), event));
const closeDialog = () => send({ type: "close" });

type Item = Extract<MenuEntry, { label: string }> & { action: () => void };

/**
 * The action entries in menu order. `details` only for the selected session
 * (the header); `context` for a row's context menu. The entries act on a
 * snapshot of the row as it is now, which the store checks against the
 * listed row when they run.
 */
export function actionEntries(s: Session, details?: Details, options: { context?: boolean } = {}): { change: Item; stop: Item & { shown: boolean }; markRead: Item; rename: Item; archive?: Item; end: Item } {
  const row = snapshotOf(s);
  const m = menuState(row, !computerOffline(s.computer), updating(s.computer, s.name), details,
    { context: options.context, reading: reading(s.computer, s.name), interrupting: interrupting(s.computer, s.name) });
  const hint = (enabled: boolean, reason?: string) => (enabled ? undefined : reason);
  const archived = row.state === "archived";
  return {
    change: {
      label: m.change.label, icon: archived || row.state !== "running" ? "play" : "pause", disabled: !m.change.enabled, hint: hint(m.change.enabled, m.change.reason),
      action: () => { void change(row); },
    },
    stop: {
      label: "Stop", icon: "circle-stop", shown: m.stop.visible, disabled: !m.stop.enabled, hint: hint(m.stop.enabled, m.stop.reason),
      action: () => { if (details) void perform(row, { kind: "interrupt", turn_started: details.turn_started }); },
    },
    markRead: {
      label: m.markRead.label, icon: m.markRead.label === "Mark as read" ? "check-check" : "bell", disabled: !m.markRead.enabled, hint: hint(m.markRead.enabled, m.markRead.reason),
      action: () => { void perform(row, { kind: "mark_read", reply_id: row.replyId ?? null, attention_signature: row.attentionSignature ?? null }); },
    },
    rename: { label: "Rename…", icon: "pencil", disabled: !m.rename, action: () => send({ type: "open", kind: "rename", row }) },
    archive: m.archive.visible
      ? { label: "Move to archive", icon: "archive", disabled: !m.archive.enabled, hint: hint(m.archive.enabled, m.archive.reason), action: () => { void perform(row, { kind: "archive" }); } }
      : undefined,
    end: {
      label: m.end.label, icon: m.end.label === "Terminate session…" ? "circle-stop" : "trash", danger: true, disabled: !m.end.enabled, hint: hint(m.end.enabled, m.end.reason),
      action: () => send({ type: "open", kind: "terminate", row }),
    },
  };
}

/**
 * The copy entries (Qt's context menu, 5306-5307 and 5374-5376). `details`
 * only for the selected session, whose folder and run they prefer. They
 * show no hint when off, as Qt shows none.
 */
export function copyEntries(s: Session, details?: Details): { command: Item; name: Item; reference: Item; folder: Item } {
  const row = snapshotOf(s);
  const c = copyState(row, !computerOffline(s.computer), details);
  return {
    command: { label: c.command.label, icon: "link", disabled: !c.command.enabled, action: () => { void copyCommand(row); } },
    name: { label: "Copy session name", icon: "copy", action: () => { void copyName(row); } },
    reference: { label: "Copy session reference", icon: "link", disabled: !c.reference, action: () => { void copyReference(row, details); } },
    folder: { label: "Copy folder path", icon: "copy", disabled: !c.folder.enabled, action: () => { void copyFolder(row, details); } },
  };
}

/** The listed rows, for the rename dialog's check of names in use. */
function* listedRows(): Iterable<Session> {
  for (const id of catalog.order) {
    const row = catalog.rows[id];
    if (row) yield row;
  }
}

/** Qt's rename dialog: the agent and project stay; only the tag after them is edited. */
function RenameDialog(props: { row: ActionRow; close: () => void }) {
  const { prefix, tag: oldTag } = renameParts(props.row.name);
  const [value, setValue] = createSignal(oldTag);
  // Checked as the text changes; the listed names are read then, not followed.
  const problem = createMemo(() => {
    const tag = value();
    return untrack(() => renameProblem(tag, props.row, listedRows()));
  });
  const submit = (e?: Event) => {
    e?.preventDefault();
    const tag = value();
    if (tag === oldTag || renameProblem(tag, props.row, listedRows())) return;
    props.close();
    void perform(props.row, { kind: "rename", new_name: prefix + tag, archive_id: props.row.archiveId ?? null });
  };
  return (
    <form onSubmit={submit}>
      <div class="dlg-head"><Icon name="pencil" size={18} class="accent" /><h2 id="action-dlg-title">Rename session</h2></div>
      <div class="dlg-body">
        <div class="dlg-detail dlg-context">{props.row.machine} / {prefix.slice(0, -1)}</div>
        <label class="dlg-label" for="rename-tag">Session name</label>
        <input id="rename-tag" data-initial class="dlg-input" placeholder="e.g. release planning" value={value()} onInput={(e) => setValue(e.currentTarget.value)} />
        <div class="dlg-detail ell" title={prefix + value()}>{prefix + value()}</div>
        <Show when={problem()}><div class="dlg-error">{problem()}</div></Show>
      </div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn" onClick={props.close}>Cancel</button>
        <button type="submit" class="btn primary" disabled={!!problem() || value() === oldTag}>Rename</button>
      </div>
    </form>
  );
}

function TerminateDialog(props: { row: ActionRow; close: () => void }) {
  const copy = terminateCopy(props.row);
  return (
    <>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="action-dlg-title">{copy.title}</h2></div>
      <div class="dlg-body">
        <div class="confirm-body" id="action-dlg-body">{copy.text}</div>
        <div class="dlg-detail">{copy.detail}</div>
      </div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn primary" data-initial autofocus onClick={props.close}>Cancel</button>
        <button type="button" class="btn solid-danger" onClick={() => { props.close(); void end(props.row, copy.action); }}>{copy.confirm}</button>
      </div>
    </>
  );
}

/**
 * One modal host for the two dialogs. In one code path it opens the modal,
 * then focuses the dialog's initial control (the tag field with its text
 * selected, or Cancel). Escape closes through the dialog's own cancel.
 */
export function ActionDialogs() {
  let el: HTMLDialogElement | undefined;
  createEffect(() => {
    const state = open();
    if (!el) return;
    if (!state) { if (el.open) el.close(); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => {
      const initial = el?.querySelector<HTMLElement>("[data-initial]");
      initial?.focus();
      if (state.kind === "rename" && initial instanceof HTMLInputElement) initial.select();
    });
  });
  return (
    <dialog class="dlg" ref={el} role={open()?.kind === "terminate" ? "alertdialog" : "dialog"} aria-labelledby="action-dlg-title"
      aria-describedby={open()?.kind === "terminate" ? "action-dlg-body" : undefined} style={{ width: "460px" }} onClose={closeDialog}>
      <Show when={open()} keyed>
        {(p) => (p.kind === "rename" ? <RenameDialog row={p.row} close={closeDialog} /> : <TerminateDialog row={p.row} close={closeDialog} />)}
      </Show>
    </dialog>
  );
}

/** The action store's notices, as toasts. Errors and warnings stay until dismissed. */
export function ActionNotices() {
  return (
    <div class="toasts" aria-live="polite" style={{ bottom: "auto", top: "16px" }}>
      <For each={notices()}>
        {(n) => (
          <div class={`toast ${n.tone}`}>
            <Show when={n.sticky}><button class="icon-btn dismiss" aria-label="Dismiss" onClick={() => dismiss(n.id)}><Icon name="x" size={14} /></button></Show>
            <span class="note">{n.text}</span>
          </div>
        )}
      </For>
    </div>
  );
}
