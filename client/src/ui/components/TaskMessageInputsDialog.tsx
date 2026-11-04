import { For, Match, Show, Switch, createSignal, onCleanup, onMount } from "solid-js";
import { createHeldReview, heldInputs, type HeldAnswer, type HeldInputsSource, type HeldRow } from "../../store/heldInputs";
import { formatDataSize } from "../screens/settings/files-size";
import { tr } from "../../i18n";
import "../styles/held-copies.css";

const createdAt = (ms: number) => new Date(ms).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" });
const stateText = (row: HeldRow) => row.state === "orphaned" ? tr("Not sent") : tr("Sent; copy not removed");
const fileText = (row: HeldRow) => row.file === "present" ? tr("File present") : row.file === "missing" ? tr("File missing") : tr("Cannot be checked");
const outcomeText = (answer: HeldAnswer) => answer === "removed" ? tr("Copy removed")
  : answer === "released" ? tr("Record released")
  : answer === "changed" ? tr("Kept: the file changed.")
  : answer === "kept" ? tr("Kept: nothing was changed.")
  : answer === "not_sent" ? tr("Nothing was sent.")
  : tr("Result unknown.");

/** The background service's held task message copies: one bounded page per
 * click, each release confirmed and sent once, never retried. */
export function TaskMessageInputsDialog(props: { origin: HTMLElement; fallback?: () => HTMLElement | undefined; close(): void; source?: HeldInputsSource }) {
  const source = props.source ?? heldInputs;
  const origin = props.origin, fallback = props.fallback;
  const [confirming, setConfirming] = createSignal<{ row: HeldRow; listing: number; origin: HTMLElement }>();
  let dialog!: HTMLDialogElement, closeButton!: HTMLButtonElement, alive = true;
  const { pages, loading, failed, outcomes, read, release: send } = createHeldReview(source, () => alive);
  const release = (row: HeldRow, listing: number) => { setConfirming(undefined); void send(row, listing); };

  onMount(() => { dialog.showModal(); closeButton.focus(); void read(0, null); });
  onCleanup(() => {
    alive = false;
    if (dialog.open) dialog.close();
    // The opener goes when no copies are left; the status summary stays.
    queueMicrotask(() => { if (origin.isConnected) origin.focus(); else fallback?.()?.focus(); });
  });
  const rows = () => pages().flatMap((page) => page.rows);
  const last = () => pages().at(-1);
  return <>
    <dialog ref={dialog} class="dlg" style={{ width: "600px" }} aria-labelledby="held-copies-title" onCancel={props.close} onClose={props.close}>
      <div class="dlg-head"><h2 id="held-copies-title">{tr("Held message copies")}</h2></div>
      <div class="dlg-body">
        <p class="hint">{tr("Copies of large task messages that the background service could not remove. Removing a copy never sends or cancels the message.")}</p>
        <Show when={rows().length} fallback={
          <Show when={loading() === undefined && !failed()} fallback={<Show when={!failed()}><p class="hint">{tr("Loading…")}</p></Show>}>
            <p>{tr("No held message copies need review.")}</p>
          </Show>
        }>
          <ul class="held-copies">
            <For each={pages()}>{(page, index) => <For each={page.rows}>{(row) => {
              const outcome = () => outcomes()[row.request_id];
              return <li class="held-copy">
                <div class="held-facts">
                  <span>{createdAt(row.created)}</span>
                  <span>{formatDataSize(row.bytes)}</span>
                  <code title={row.request_id}>{row.request_id.slice(0, 8)}…</code>
                </div>
                <div class="held-facts hint"><span>{stateText(row)}</span><span>{fileText(row)}</span></div>
                <div class="held-action" aria-live="polite">
                  <Switch>
                    <Match when={row.file === "unsafe" && !outcome()}>
                      <button type="button" class="btn small ghost" disabled={loading() !== undefined} onClick={() => void read(index(), page.after)}>{tr("Review again")}</button>
                    </Match>
                    <Match when={!outcome() || outcome() === "sending"}>
                      <button type="button" class="btn small ghost" disabled={outcome() === "sending" || loading() !== undefined}
                        onClick={(event) => setConfirming({ row, listing: page.listing, origin: event.currentTarget })}>{row.file === "present" ? tr("Remove copy…") : tr("Release record…")}</button>
                    </Match>
                    <Match when={outcome() === "removed" || outcome() === "released"}>
                      <span>{outcomeText(outcome() as HeldAnswer)}</span>
                    </Match>
                    <Match when={outcome()}>{(answer) => <>
                      <span>{outcomeText(answer() as HeldAnswer)}</span>
                      <button type="button" class="btn small ghost" disabled={loading() !== undefined} onClick={() => void read(index(), page.after)}>{tr("Review again")}</button>
                    </>}</Match>
                  </Switch>
                </div>
              </li>;
            }}</For>}</For>
          </ul>
        </Show>
        <Show when={failed()}>{(failure) => <div class="held-more">
          <span class="hint">{tr("Could not read held message copies.")}</span>
          <button type="button" class="btn small ghost" onClick={() => void read(failure().index, failure().after)}>{tr("Review again")}</button>
        </div>}</Show>
        <Show when={!failed() && last()?.next}>{(next) => <div class="held-more">
          <button type="button" class="btn small ghost act-read-older" disabled={loading() !== undefined} onClick={() => void read(pages().length, next())}>{tr("Load more")}</button>
        </div>}</Show>
      </div>
      <div class="dlg-foot"><span class="spacer" /><button ref={closeButton} type="button" class="btn primary" onClick={props.close}>{tr("Close")}</button></div>
    </dialog>
    <Show when={confirming()}>{(item) => <HeldCopyConfirm row={item().row} origin={item().origin} fallback={() => closeButton}
      cancel={() => setConfirming(undefined)} confirm={() => release(item().row, item().listing)} />}</Show>
  </>;
}

/** Restates exactly what the review saw; Cancel is focused. */
function HeldCopyConfirm(props: { row: HeldRow; origin: HTMLElement; fallback(): HTMLElement; cancel(): void; confirm(): void }) {
  // Read once: after the parent's <Show> unmounts this, its props are stale.
  const row = props.row, origin = props.origin, fallback = props.fallback, remove = row.file === "present";
  let dialog!: HTMLDialogElement, cancel!: HTMLButtonElement;
  onMount(() => { dialog.showModal(); cancel.focus(); });
  onCleanup(() => {
    if (dialog.open) dialog.close();
    queueMicrotask(() => {
      const target = origin.isConnected && !(origin as HTMLButtonElement).disabled ? origin : fallback();
      if (target.isConnected) target.focus();
    });
  });
  return <dialog ref={dialog} class="dlg" style={{ width: "460px" }} aria-labelledby="held-copy-confirm-title"
    onCancel={(event) => { event.preventDefault(); props.cancel(); }} onClose={props.cancel}>
    <div class="dlg-head"><h2 id="held-copy-confirm-title">{remove ? tr("Remove copy") : tr("Release record")}</h2></div>
    <div class="dlg-body"><div class="confirm-body">
      <p>{remove
        ? tr("Remove this held copy? Zerus removes only the file it just reviewed. The message is not sent again.")
        : tr("Release this record? The copy is no longer in its folder. This frees its space; it does not confirm the copy was destroyed anywhere else.")}</p>
      <dl class="held-confirm">
        <dt>{tr("Request")}</dt><dd><code>{row.request_id}</code></dd>
        <dt>{tr("Size")}</dt><dd>{formatDataSize(row.bytes)}</dd>
        <dt>{tr("Created")}</dt><dd>{createdAt(row.created)}</dd>
        <dt>{tr("File")}</dt><dd>{fileText(row)}</dd>
      </dl>
    </div></div>
    <div class="dlg-foot"><span class="spacer" />
      <button type="button" class="btn solid-danger" onClick={props.confirm}>{remove ? tr("Remove copy") : tr("Release record")}</button>
      <button ref={cancel} type="button" class="btn primary" onClick={props.cancel}>{tr("Cancel")}</button>
    </div>
  </dialog>;
}
