import { For, Show, createEffect, createRoot, createSignal, untrack } from "solid-js";
import * as bridge from "../../bridge";
import { Icon } from "../lib/icons";
import { composer } from "../../store/composer";
import { orderedRows } from "../../store/catalog";
import { openSession } from "../../store/desk";
import { toast } from "../../store/ui";
import {
  createSavedDrafts, draftOwnerRow, modifiedText, replaceText, SECTIONS, TEXTS,
  type RestoreTarget, type SavedDrafts,
} from "../../store/savedDrafts";
import type { SavedDraftItem } from "../../types";
import type { Session } from "./types";

// The Saved drafts dialog (MessageComposer.cpp:1230-1387), opened from the
// session list's Saved drafts button (SessionsWindow.cpp:485-489). Restore
// over a draft that is not empty asks "Replace current draft?" with Cancel
// as the default; Close is the dialog's default button, as in Qt.

/** The dialog's store; its rows go when it closes. */
export const savedDrafts: SavedDrafts = createRoot(() => createSavedDrafts({
  page: bridge.savedDrafts, conflicts: bridge.draftConflicts, detail: bridge.savedDraft,
  recover: bridge.draftRecover, restore: bridge.draftRestore,
}));

/** A row's element id, for the list's active descendant. */
const rowIds = new WeakMap<SavedDraftItem, string>();
let rowCount = 0;
const rowId = (item: SavedDraftItem) => {
  let id = rowIds.get(item);
  if (!id) rowIds.set(item, (id = `saved-draft-${++rowCount}`));
  return id;
};

/** The open composer as Restore sees it. */
function restoreTarget(): RestoreTarget | undefined {
  const v = composer.view();
  if (!v) return undefined;
  return {
    handle: v.handle, key: v.key,
    hasDraft: composer.text() !== "" || v.attachments.length > 0,
    idle: v.loaded && !v.sending && !v.waiting && !composer.operationLocked(),
  };
}

function Row(props: { item: SavedDraftItem; onOpen: () => void }) {
  const on = () => savedDrafts.selected() === props.item;
  return (
    <div class="draft-row" classList={{ on: on() }} role="option" aria-selected={on()} aria-label={props.item.title}
      id={rowId(props.item)}
      onClick={() => { if (!on()) savedDrafts.select(props.item); }} onDblClick={() => props.onOpen()}>
      <div class="draft-row-head">
        <b class="ell">{props.item.caption}</b>
        <span class="grow" />
        <span class="draft-machine ell">{props.item.machine}</span>
      </div>
      <Show when={props.item.meta}><div class="draft-row-meta ell">{props.item.meta}</div></Show>
      <div class="draft-row-foot">
        <span class="ell">{props.item.detail}</span>
        <span class="grow" />
        <Show when={props.item.modified > 0}><span class="act-time">{modifiedText(props.item.modified)}</span></Show>
      </div>
    </div>
  );
}

/** "Replace current draft?": Cancel has the focus and is the default; Escape is Cancel. */
function ReplaceDialog(props: { open: boolean; text: string; onCancel: () => void; onConfirm: () => void }) {
  let el: HTMLDialogElement | undefined;
  createEffect(() => {
    if (!el) return;
    if (!props.open) { if (el.open) el.close(); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => el?.querySelector<HTMLElement>("[data-initial]")?.focus());
  });
  return (
    <dialog class="dlg" ref={el} role="alertdialog" aria-labelledby="replace-draft-title" aria-describedby="replace-draft-body" style={{ width: "460px" }}
      onCancel={(e) => { e.preventDefault(); props.onCancel(); }} onClose={() => { if (!el?.open && props.open) props.onCancel(); }}>
      <div class="dlg-head"><Icon name="triangle-alert" size={18} class="accent" /><h2 id="replace-draft-title">{TEXTS.replaceTitle}</h2></div>
      <div class="dlg-body"><div class="confirm-body" id="replace-draft-body" style={{ "white-space": "pre-wrap" }}>{props.text}</div></div>
      <div class="dlg-foot">
        <span class="spacer" />
        <button type="button" class="btn primary" data-initial onClick={props.onCancel}>Cancel</button>
        <button type="button" class="btn solid-danger" onClick={props.onConfirm}>{TEXTS.replaceConfirm}</button>
      </div>
    </dialog>
  );
}

export function SavedDraftsDialog() {
  let el: HTMLDialogElement | undefined;
  let list: HTMLDivElement | undefined;
  const [replacing, setReplacing] = createSignal<string>();
  /** The selected draft's session row, found when it is selected (Qt's canOpenDraftSession). */
  const [owner, setOwner] = createSignal<Session>();
  createEffect(() => {
    const item = savedDrafts.selected();
    setOwner(item ? untrack(() => draftOwnerRow(item, orderedRows())) : undefined);
    // The selected row stays in view.
    if (item) queueMicrotask(() => list?.querySelector(".draft-row.on")?.scrollIntoView({ block: "nearest" }));
  });
  createEffect(() => {
    if (!el) return;
    if (!savedDrafts.open()) { if (el.open) el.close(); setReplacing(undefined); return; }
    if (!el.open) el.showModal();
    queueMicrotask(() => el?.querySelector<HTMLElement>("[data-initial]")?.focus());
  });

  const all = () => SECTIONS.flatMap(({ id }) => savedDrafts.sections[id].items);
  const move = (step: number) => {
    const items = all();
    if (!items.length) return;
    const at = items.indexOf(savedDrafts.selected()!);
    const next = items[Math.max(0, Math.min(items.length - 1, at < 0 ? 0 : at + step))];
    if (next !== savedDrafts.selected()) savedDrafts.select(next);
  };

  const goTo = () => {
    const item = savedDrafts.selected();
    // The row is looked for again: the list may have changed since the selection.
    const row = item && draftOwnerRow(item, orderedRows());
    if (!row) return;
    savedDrafts.close();
    openSession({ computer: row.computer, key: row.catalogKey });
  };
  const restore = async (confirmed = false) => {
    const target = restoreTarget();
    if (!target || !savedDrafts.canRestore(target)) return;
    if (!confirmed && savedDrafts.asks(target)) { setReplacing(target.key); return; }
    // What was typed reaches Rust first; Restore replaces it with a new text epoch.
    await composer.flush();
    if (await savedDrafts.restore(restoreTarget())) savedDrafts.close();
  };
  const copy = () => {
    const text = savedDrafts.detail()?.text ?? "";
    if (!text) return;
    void navigator.clipboard?.writeText(text).then(() => toast("Copied", "The draft's text is on the clipboard"), () => toast("Not copied", "The clipboard is unavailable"));
  };

  const detailLine = () => {
    if (!savedDrafts.selected()) return "";
    return savedDrafts.detail()?.detail ?? TEXTS.loading;
  };
  return (
    <dialog class="dlg saved-drafts" ref={el} aria-labelledby="saved-drafts-title" style={{ width: "720px" }}
      onCancel={(e) => { e.preventDefault(); savedDrafts.close(); }} onClose={() => { if (!el?.open && savedDrafts.open()) savedDrafts.close(); }}>
      <div class="dlg-head"><Icon name="drafts" size={18} class="accent" /><h2 id="saved-drafts-title">{TEXTS.title}</h2></div>
      <div class="dlg-body saved-drafts-body">
        <p class="hint">{savedDrafts.empty() ? TEXTS.empty : TEXTS.hint}</p>
        <div class="draft-list" ref={list} role="listbox" tabindex="0" aria-label={TEXTS.title}
          aria-activedescendant={savedDrafts.selected() ? rowId(savedDrafts.selected()!) : undefined}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); move(e.key === "ArrowDown" ? 1 : -1); }
          }}>
          <For each={SECTIONS}>{(section) => (
            <Show when={savedDrafts.sections[section.id].items.length > 0}>
              <div class="draft-section" role="group" aria-label={section.title}>
                <div class="eyebrow draft-section-title">{section.title}</div>
                <Show when={section.note}><p class="hint draft-section-note">{section.note}</p></Show>
                <For each={savedDrafts.sections[section.id].items}>{(item) => <Row item={item} onOpen={goTo} />}</For>
                <Show when={savedDrafts.sections[section.id].next !== null}>
                  <button type="button" class="btn small draft-more" disabled={savedDrafts.sections[section.id].loading}
                    onClick={() => { void savedDrafts.more(section.id); }}>Show more</button>
                </Show>
              </div>
            </Show>
          )}</For>
        </div>
        <pre class="draft-preview mono" aria-label="Draft text">{savedDrafts.detail()?.text ?? ""}</pre>
        <p class="hint draft-detail">{detailLine()}</p>
        <Show when={savedDrafts.message()}>{(m) => <p class="draft-message" classList={{ error: m().error }}>{m().text}</p>}</Show>
      </div>
      <div class="dlg-foot">
        <button type="button" class="btn" disabled={!savedDrafts.detail()?.text} onClick={copy}>Copy text</button>
        <button type="button" class="btn" disabled={!savedDrafts.canRestore(restoreTarget())} onClick={() => { void restore(); }}>Restore in current session</button>
        <button type="button" class="btn" disabled={!owner()} title={!owner() && savedDrafts.selected() ? TEXTS.unreachable : undefined} onClick={goTo}>Go to session</button>
        <Show when={savedDrafts.selected()?.source === "qt"}>
          <button type="button" class="btn" disabled={savedDrafts.busy()} onClick={() => { void savedDrafts.recover(); }}>Recover</button>
        </Show>
        <span class="spacer" />
        <button type="button" class="btn primary" data-initial onClick={() => savedDrafts.close()}>Close</button>
      </div>
      <ReplaceDialog open={!!replacing()} text={replaceText(replacing() ?? "")}
        onCancel={() => setReplacing(undefined)} onConfirm={() => { setReplacing(undefined); void restore(true); }} />
    </dialog>
  );
}
