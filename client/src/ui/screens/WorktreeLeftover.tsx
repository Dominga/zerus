import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { MachineLabel } from "../components/MachineLabel";
import { tr } from "../../i18n";
import type { Overview, OverviewRow, Settled } from "./worktree-overview-state";

const leaf = (p: string) => p.split(/[\\/]/u).filter(Boolean).at(-1) ?? p;
const branchState = (retained: boolean | null) => retained === true ? tr("Branch kept") : retained === false ? tr("Branch moved since the removal") : tr("Branch not checked");

/** A partially removed folder: what remains and the two explicit actions.
 * No file browsing; the owning computer reviews the folder again before any action. */
export function LeftoverPanel(props: { row: OverviewRow; busy: boolean; keep(): void; remove(): void; review(): void }) {
  const item = () => props.row.leftover!;
  const done = () => !!item().outcome;
  const deletable = () => item().deletable || item().consent;
  return <section class="su-insp wtl-panel" aria-label={tr("Partially removed folder")}>
    <header class="ip-head"><h3 class="ell" title={props.row.path}>{leaf(props.row.path)}</h3></header>
    <p class="ip-brief mono ell" title={props.row.path}>{props.row.path}</p>
    <div class="notice warn"><Icon name="triangle-alert" size={14} /><span>{item().repository ? tr("Git no longer tracks this folder, but files remain. The removal is never retried.") : tr("Repository missing. Keep the folder or clean it up yourself.")}</span></div>
    <dl class="wtl-facts">
      <dt>{tr("Computer")}</dt><dd><MachineLabel alias={props.row.machine} class="mbadge" /></dd>
      <dt>{tr("Branch")}</dt><dd><span class="mono">{item().branch || "—"}</span><span class="dim">{branchState(item().branch_retained)}</span></dd>
      <dt>{tr("Remaining files")}</dt><dd class="tabular">{props.row.size}</dd>
      <dt>{tr("Removal stopped")}</dt><dd>{item().interrupted ? tr("The removal was interrupted.") : item().stopped || "—"}</dd>
      <Show when={item().sessions.length}><dt>{tr("Sessions")}</dt><dd>{item().sessions.join(", ")}</dd></Show>
      <Show when={item().processes}><dt>{tr("Running programs")}</dt><dd class="tabular">{item().processes}</dd></Show>
    </dl>
    <Show when={item().reasons.length && !done()}><div class="wtl-reasons"><b>{tr("Deleting is blocked")}</b><ul><For each={item().reasons}>{(reason) => <li>{reason}</li>}</For></ul></div></Show>
    <Show when={item().outcome}>{(outcome) => <div class="notice" classList={{ warn: outcome().outcome === "unknown" || outcome().outcome === "refused" }} role="status"><span><b>{props.row.state}</b> {props.row.remains}</span></div>}</Show>
    <span class="grow" />
    <div class="row wtl-actions"><span class="grow" />
      <Show when={!done()} fallback={<button class="btn" disabled={props.busy} onClick={() => props.review()}><Icon name="refresh-cw" size={13} />{tr("Review again")}</button>}>
        <button class="btn" disabled={props.busy} onClick={() => props.keep()}>{tr("Keep folder…")}</button>
        <Show when={item().repository}><button class="btn danger" disabled={props.busy || !deletable()} title={deletable() ? undefined : tr("Resolve what blocks deleting, then review again.")} onClick={() => props.remove()}><Icon name="trash" size={13} />{tr("Delete remaining files…")}</button></Show>
      </Show>
    </div>
  </section>;
}

/** The explicit confirmation of one leftover action. Delete states that the
 * files cannot be restored and the branch stays; Cancel has the default focus
 * and the process acceptance appears only when the review offered it. One
 * call per confirmation; an unknown result offers Review again, never a retry. */
export function LeftoverDialog(props: { row: OverviewRow; mode: "keep" | "delete"; state: Overview; close(review: boolean): void }) {
  let dialog!: HTMLDialogElement, cancel!: HTMLButtonElement;
  const origin = document.activeElement;
  const item = props.row.leftover!;
  const remove = props.mode === "delete";
  const [consent, setConsent] = createSignal(false);
  const [running, setRunning] = createSignal(false);
  const [result, setResult] = createSignal<Settled | string>();
  const unknown = () => { const value = result(); return typeof value === "string" || value?.outcome === "unknown"; };
  onMount(() => { dialog.showModal(); queueMicrotask(() => cancel.focus()); });
  onCleanup(() => { dialog.close(); if (origin instanceof HTMLElement && origin.isConnected) origin.focus(); });
  const run = async () => {
    if (running() || result() !== undefined || (remove && item.consent && !consent())) return;
    setRunning(true);
    try { setResult(await props.state.leftover(props.row, remove ? consent() : null)); } finally { setRunning(false); queueMicrotask(() => cancel?.focus()); }
  };
  const finish = () => { if (!running()) props.close(result() !== undefined && !unknown()); };
  const message = () => {
    const value = result();
    if (typeof value === "string") return [tr("Result unknown"), value, tr("Review again before deciding what to do next.")];
    switch (value?.outcome) {
      case "deleted": return [tr("Files deleted"), tr("The remaining files were deleted. The branch is kept.")];
      case "forgotten": return [tr("Folder kept"), tr("Zerus no longer tracks this folder. Its files and the branch stay.")];
      case "resolved": return [tr("Record already gone"), tr("Zerus no longer tracks this folder. Review again to see what remains.")];
      case "refused": return [tr("Nothing changed"), value.detail];
      case "unknown": return [tr("Result unknown"), value.detail, tr("An unknown result will not be retried.")];
      default: return [];
    }
  };
  return <dialog ref={dialog} class="dlg su-dlg worktree-leftover" role="alertdialog" aria-labelledby="wtl-title" aria-describedby="wtl-text" style={{ width: "min(560px, calc(100vw - 48px))", position: "fixed", inset: "0", margin: "auto" }} onCancel={(e) => { e.preventDefault(); finish(); }}>
    <div class="dlg-head"><Icon name={remove ? "trash" : "folder"} size={18} class={remove ? "danger" : "accent"} /><h2 id="wtl-title">{remove ? tr("Delete remaining files") : tr("Keep folder")}</h2></div>
    <div class="dlg-body wtl-body" id="wtl-text">
      <Show when={result() === undefined} fallback={<div class="notice" classList={{ warn: unknown() || (result() as Settled).outcome === "refused" }} role="status"><span><For each={message()}>{(line, i) => i() === 0 ? <b>{line}</b> : <p>{line}</p>}</For></span></div>}>
        <Show when={remove} fallback={<p>{tr("Zerus forgets the failed removal and stops listing this folder. Its files and the branch stay where they are.")}</p>}>
          <div class="notice danger"><Icon name="triangle-alert" size={14} /><span><b>{tr("These files will be permanently deleted.")}</b> {tr("They cannot be restored.")}</span></div>
          <p>{item.branch ? tr("The branch %1 is kept.", item.branch) : tr("No branch is changed.")}</p>
        </Show>
        <dl class="wtl-facts"><dt>{tr("Folder")}</dt><dd class="mono wtl-path">{props.row.path}</dd><dt>{tr("Computer")}</dt><dd><MachineLabel alias={props.row.machine} class="mbadge" /></dd><Show when={remove}><dt>{tr("Remaining files")}</dt><dd class="tabular">{props.row.size}</dd></Show></dl>
        <Show when={remove && item.consent}><label class="check" title={tr("Accept this uncertainty for this folder for five minutes. Known programs and sessions using the folder still prevent deletion.")}><input type="checkbox" checked={consent()} disabled={running()} onChange={(e) => setConsent(e.currentTarget.checked)} />{tr("Delete even if some running programs cannot be checked")}</label></Show>
      </Show>
      <Show when={running()}><progress aria-label={remove ? tr("Deleting remaining files") : tr("Keeping the folder")} /></Show>
    </div>
    <div class="dlg-foot"><span class="grow" />
      <Show when={result() === undefined} fallback={<><Show when={unknown()}><button type="button" class="btn" onClick={() => props.close(true)}><Icon name="refresh-cw" size={13} />{tr("Review again")}</button></Show><button type="button" ref={cancel} class="btn primary" onClick={finish}>{tr("Close")}</button></>}>
        <button type="button" ref={cancel} class="btn" disabled={running()} onClick={finish}>{tr("Cancel")}</button>
        <Show when={remove} fallback={<button type="button" class="btn primary" disabled={running()} onClick={() => void run()}>{tr("Keep folder")}</button>}>
          <button type="button" class="btn solid-danger" disabled={running() || (item.consent && !consent())} onClick={() => void run()}>{tr("Delete files")}</button>
        </Show>
      </Show>
    </div>
  </dialog>;
}
