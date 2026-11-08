import { createEffect, createSignal } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import type { InboxRemoveReply } from "../../types";
import { REMOVE_TEXT, REMOVE_TITLE, removeNotice, removeState } from "./inbox-actions";
import type { InboxItem } from "./types";

/** What Remove reads and calls: the composer's view and its Rust command, the window notices, and where the focus goes after Remove. */
export interface InboxRemovePorts {
  view(): { handle: number; inbox_removing?: string | null; inbox_busy?: boolean } | undefined;
  remove(handle: number, run: string, conversation: string, messageId: string): Promise<InboxRemoveReply>;
  notify(tone: "error" | "info", text: string, sticky: boolean): void;
  done(): void;
}

/** The message whose Remove waits for its confirmation, the composer it was
 * shown in, and the Remove button the focus returns to on Cancel. */
const [asking, setAsking] = createSignal<{ item: InboxItem; handle: number; from: HTMLElement }>();

/** Remove under a not-delivered OpenCode message (ActivityView.cpp:1077-1087): off while any removal runs. */
export function InboxRemove(props: { item: InboxItem; ports: InboxRemovePorts }) {
  const state = () => removeState(props.item, props.ports.view());
  const open = (from: HTMLElement) => {
    const handle = props.ports.view()?.handle;
    if (state().disabled || handle === undefined) return;
    setAsking({ item: props.item, handle, from });
  };
  return (
    <div class="act-local-actions">
      <button type="button" class="btn small" aria-disabled={state().disabled ? "true" : undefined}
        onClick={(e) => open(e.currentTarget)}>{state().label}</button>
    </div>
  );
}

/**
 * "Remove queued message": Cancel has the focus and is the default; Escape is
 * Cancel. Remove acts only on the message, composer, run and conversation the
 * dialog opened for; Rust checks them again and sends nothing when they
 * changed (SessionsWindow::removeQueuedMessage, 4433-4467). The dialog stays
 * open when the message changes underneath it, as in Qt.
 */
export function RemoveQueuedDialog(props: { ports: InboxRemovePorts }) {
  let el: HTMLDialogElement | undefined;
  createEffect(() => {
    if (!el) return;
    if (!asking()) { if (el.open) el.close(); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => el?.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  const cancel = () => {
    const from = asking()?.from;
    setAsking(undefined);
    if (from?.isConnected) from.focus();
  };
  const confirm = () => {
    const shown = asking();
    setAsking(undefined);
    props.ports.done();
    if (!shown) return;
    void props.ports.remove(shown.handle, shown.item.run, shown.item.conversation, shown.item.messageId).then((reply) => {
      const notice = removeNotice(reply);
      if (notice) props.ports.notify(notice.tone, notice.text, notice.sticky);
    });
  };
  return (
    <dialog class="dlg" ref={el} role="alertdialog" aria-labelledby="inbox-remove-title" aria-describedby="inbox-remove-body" style={{ width: "460px" }}
      onCancel={(e) => { e.preventDefault(); cancel(); }} onClose={() => { if (!el?.open && asking()) cancel(); }}>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="danger" /><h2 id="inbox-remove-title">{tr(REMOVE_TITLE)}</h2></div>
      <div class="dlg-body"><div class="confirm-body" id="inbox-remove-body">{tr(REMOVE_TEXT)}</div></div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn solid-danger" data-confirm onClick={confirm}>{tr("Remove")}</button>
        <button type="button" class="btn primary" data-initial onClick={cancel}>{tr("Cancel")}</button>
      </div>
    </dialog>
  );
}
