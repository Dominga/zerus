import { For, Show, createEffect, createSignal } from "solid-js";
import { Icon } from "../lib/icons";
import { computerOffline } from "../../store/catalog";
import { dismiss, notices, perform, updating } from "../../store/actions";
import type { MenuEntry } from "../../store/ui";
import type { Details, RowOut } from "../../types";
import type { Session } from "./types";
import { menuState, renameError, terminateCopy } from "./actions-ui";

// The guarded session actions: the entries both menus share, the Rename and
// Terminate dialogs, and the notices the action store raises.

type Pending = { kind: "rename" | "terminate"; entry: RowOut };
const [open, setOpen] = createSignal<Pending | undefined>();

/** The core's row for a catalog session; undefined until it carries one. */
export const entryOf = (s: Session): RowOut | undefined => (s.view ? { computer: s.computer, key: s.catalogKey, row: s.view } : undefined);

/** The state of a session's action entries, with reactive inputs. */
export function stateOf(s: Session, details?: Details) {
  const entry = entryOf(s);
  return entry ? { entry, state: menuState(entry, !computerOffline(s.computer), updating(s.computer, s.name), details) } : undefined;
}

type Item = Extract<MenuEntry, { label: string }> & { action: () => void };

/** The action entries in menu order; `details` only for the selected session. */
export function actionEntries(s: Session, details?: Details): { change: Item; stop: Item; markRead: Item; rename: Item; archive?: Item; end: Item } | undefined {
  const got = stateOf(s, details);
  if (!got) return undefined;
  const { entry, state: m } = got;
  const row = entry.row;
  const hint = (enabled: boolean, reason?: string) => (enabled ? undefined : reason);
  const archived = row.state === "archived";
  return {
    change: {
      label: m.change.label, icon: archived || row.state !== "running" ? "play" : "pause", disabled: !m.change.enabled, hint: hint(m.change.enabled, m.change.reason),
      action: () => { void perform(entry, { kind: row.state === "running" ? "pause" : "resume" }); },
    },
    stop: {
      label: "Stop", icon: "circle-stop", disabled: !m.stop,
      action: () => { void perform(entry, { kind: "interrupt", turn_started: row.turn_started ?? 0 }); },
    },
    markRead: {
      label: m.markRead.label, icon: m.markRead.label === "Mark as read" ? "check-check" : "bell", disabled: !m.markRead.enabled, hint: hint(m.markRead.enabled, m.markRead.reason),
      action: () => { void perform(entry, { kind: "mark_read", reply_id: row.reply_id ?? null, attention_signature: row.attention_signature ?? null }); },
    },
    rename: { label: "Rename…", icon: "pencil", disabled: !m.rename, action: () => setOpen({ kind: "rename", entry }) },
    archive: m.archive.visible
      ? { label: "Move to archive", icon: "archive", disabled: !m.archive.enabled, hint: hint(m.archive.enabled, m.archive.reason), action: () => { void perform(entry, { kind: "archive" }); } }
      : undefined,
    end: {
      label: m.end.label, icon: m.end.label === "Terminate session…" ? "circle-stop" : "trash", danger: true, disabled: !m.end.enabled, hint: hint(m.end.enabled, m.end.reason),
      action: () => setOpen({ kind: "terminate", entry }),
    },
  };
}

function RenameDialog(props: { entry: RowOut; close: () => void }) {
  const current = props.entry.row.name;
  const [value, setValue] = createSignal(current);
  let input: HTMLInputElement | undefined;
  const error = () => renameError(value());
  const unchanged = () => value() === current;
  createEffect(() => {
    input?.focus();
    input?.setSelectionRange(current.indexOf("/") + 1, current.length);
  });
  const submit = (e?: Event) => {
    e?.preventDefault();
    if (error() || unchanged()) return;
    props.close();
    void perform(props.entry, { kind: "rename", new_name: value(), archive_id: props.entry.row.archive_id ?? null });
  };
  return (
    <form onSubmit={submit}>
      <div class="dlg-head"><Icon name="pencil" size={18} class="accent" /><h2 id="action-dlg-title">Rename session</h2></div>
      <div class="dlg-body">
        <input ref={input} class="dlg-input" aria-label="Session name" value={value()} onInput={(e) => setValue(e.currentTarget.value)} />
        <Show when={error() && !unchanged()}><div class="dlg-error">{error()}</div></Show>
      </div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn" onClick={props.close}>Cancel</button>
        <button type="submit" class="btn primary" disabled={!!error() || unchanged()}>Rename</button>
      </div>
    </form>
  );
}

function TerminateDialog(props: { entry: RowOut; close: () => void }) {
  const copy = terminateCopy(props.entry);
  return (
    <>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="action-dlg-title">{copy.title}</h2></div>
      <div class="dlg-body">
        <div class="confirm-body">{copy.text}</div>
        <div class="dlg-detail">{copy.detail}</div>
      </div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn primary" autofocus onClick={props.close}>Cancel</button>
        <button type="button" class="btn solid-danger" onClick={() => { props.close(); void perform(props.entry, { kind: "terminate" }); }}>{copy.confirm}</button>
      </div>
    </>
  );
}

/** One modal host for the two dialogs; Escape cancels through the dialog's own close. */
export function ActionDialogs() {
  let el: HTMLDialogElement | undefined;
  createEffect(() => {
    if (!el) return;
    if (open()) { if (!el.open) el.showModal(); } else if (el.open) el.close();
  });
  return (
    <dialog class="dlg" ref={el} aria-labelledby="action-dlg-title" style={{ width: "460px" }} onClose={() => setOpen(undefined)}>
      <Show when={open()}>
        {(p) => (p().kind === "rename" ? <RenameDialog entry={p().entry} close={() => setOpen(undefined)} /> : <TerminateDialog entry={p().entry} close={() => setOpen(undefined)} />)}
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
            <span>{n.text}</span>
          </div>
        )}
      </For>
    </div>
  );
}
